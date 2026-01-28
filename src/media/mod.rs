//! Media player integration for Now Playing display
//!
//! Supports two sources:
//! 1. MPRIS2 on session bus (local players like VLC, Firefox)
//! 2. BlueZ MediaPlayer1 on system bus (Bluetooth A2DP from phones)
//!
//! BlueZ is checked first since Bluetooth audio is the primary use case.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use zbus::blocking::Connection;
use zbus::zvariant::{OwnedValue, Value};

/// Playback status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaybackStatus {
    Playing,
    Paused,
    #[default]
    Stopped,
}

impl PlaybackStatus {
    fn from_str(s: &str) -> Self {
        match s {
            "Playing" => Self::Playing,
            "Paused" => Self::Paused,
            _ => Self::Stopped,
        }
    }
}

/// Media info from MPRIS
#[derive(Debug, Clone, Default)]
pub struct MediaInfo {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub status: PlaybackStatus,
    pub player_name: String,
}

impl MediaInfo {
    /// Check if media is active (playing or paused with track info)
    pub fn is_active(&self) -> bool {
        self.status != PlaybackStatus::Stopped && !self.title.is_empty()
    }
}

/// Player source type
#[derive(Debug, Clone, PartialEq)]
enum PlayerSource {
    Mpris(String),      // MPRIS player name (session bus)
    Bluez(String),      // BlueZ device path (system bus)
}

/// Thread-safe media state container
pub struct MediaState {
    info: Mutex<MediaInfo>,
    changed: AtomicBool,
    running: AtomicBool,
    // Connection for control methods (lazy initialized)
    session_conn: Mutex<Option<Connection>>,
    system_conn: Mutex<Option<Connection>>,
    current_player: Mutex<Option<PlayerSource>>,
}

impl MediaState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            info: Mutex::new(MediaInfo::default()),
            changed: AtomicBool::new(false),
            running: AtomicBool::new(true),
            session_conn: Mutex::new(None),
            system_conn: Mutex::new(None),
            current_player: Mutex::new(None),
        })
    }

    /// Stop monitoring
    pub fn stop(&self) {
        self.running.store(false, Ordering::Release);
    }

    /// Check if state changed and reset flag
    pub fn take_changed(&self) -> bool {
        self.changed.swap(false, Ordering::AcqRel)
    }

    /// Get current media info
    pub fn get(&self) -> MediaInfo {
        self.info.lock().unwrap().clone()
    }

    /// Update media info (internal)
    fn update(&self, info: MediaInfo, source: Option<PlayerSource>) {
        let mut current = self.info.lock().unwrap();
        if current.title != info.title
            || current.artist != info.artist
            || current.status != info.status
        {
            *current = info;
            self.changed.store(true, Ordering::Release);
        }
        if let Some(src) = source {
            *self.current_player.lock().unwrap() = Some(src);
        }
    }

    /// Set connections for control methods
    fn set_connections(&self, session: Option<Connection>, system: Option<Connection>) {
        if let Some(c) = session {
            *self.session_conn.lock().unwrap() = Some(c);
        }
        if let Some(c) = system {
            *self.system_conn.lock().unwrap() = Some(c);
        }
    }

    /// Play/Pause toggle
    pub fn play_pause(&self) {
        let player = self.current_player.lock().unwrap().clone();
        match player {
            Some(PlayerSource::Mpris(name)) => {
                if let Some(conn) = self.session_conn.lock().unwrap().as_ref() {
                    let _ = call_mpris_method(conn, &name, "PlayPause");
                }
            }
            Some(PlayerSource::Bluez(path)) => {
                if let Some(conn) = self.system_conn.lock().unwrap().as_ref() {
                    // Try pause first, then play (BlueZ doesn't have PlayPause)
                    let info = self.info.lock().unwrap();
                    let method = if info.status == PlaybackStatus::Playing { "Pause" } else { "Play" };
                    drop(info);
                    let _ = call_bluez_method(conn, &path, method);
                }
            }
            None => {}
        }
    }

    /// Next track
    pub fn next(&self) {
        let player = self.current_player.lock().unwrap().clone();
        match player {
            Some(PlayerSource::Mpris(name)) => {
                if let Some(conn) = self.session_conn.lock().unwrap().as_ref() {
                    let _ = call_mpris_method(conn, &name, "Next");
                }
            }
            Some(PlayerSource::Bluez(path)) => {
                if let Some(conn) = self.system_conn.lock().unwrap().as_ref() {
                    let _ = call_bluez_method(conn, &path, "Next");
                }
            }
            None => {}
        }
    }

    /// Previous track
    pub fn previous(&self) {
        let player = self.current_player.lock().unwrap().clone();
        match player {
            Some(PlayerSource::Mpris(name)) => {
                if let Some(conn) = self.session_conn.lock().unwrap().as_ref() {
                    let _ = call_mpris_method(conn, &name, "Previous");
                }
            }
            Some(PlayerSource::Bluez(path)) => {
                if let Some(conn) = self.system_conn.lock().unwrap().as_ref() {
                    let _ = call_bluez_method(conn, &path, "Previous");
                }
            }
            None => {}
        }
    }
}

/// Call a method on MPRIS player interface
fn call_mpris_method(conn: &Connection, player: &str, method: &str) -> zbus::Result<()> {
    conn.call_method(
        Some(player),
        "/org/mpris/MediaPlayer2",
        Some("org.mpris.MediaPlayer2.Player"),
        method,
        &(),
    )?;
    Ok(())
}

/// Call a method on BlueZ MediaPlayer interface
fn call_bluez_method(conn: &Connection, path: &str, method: &str) -> zbus::Result<()> {
    conn.call_method(
        Some("org.bluez"),
        path,
        Some("org.bluez.MediaPlayer1"),
        method,
        &(),
    )?;
    Ok(())
}

/// Start background thread for media monitoring
pub fn start_media_thread(state: Arc<MediaState>) {
    std::thread::spawn(move || {
        // Connect to both buses
        let system_conn = Connection::system().ok();
        let session_conn = Connection::session().ok();

        if system_conn.is_none() && session_conn.is_none() {
            eprintln!("Failed to connect to any D-Bus");
            return;
        }

        // Store connections for control methods
        state.set_connections(
            session_conn.as_ref().and_then(|_| Connection::session().ok()),
            system_conn.as_ref().and_then(|_| Connection::system().ok()),
        );

        // Poll interval - use longer interval to avoid overwhelming UI
        let poll_interval = std::time::Duration::from_millis(1000);

        while state.running.load(Ordering::Acquire) {
            // Check BlueZ first (Bluetooth audio is primary use case)
            if let Some(conn) = &system_conn {
                if let Some((path, info)) = find_bluez_player(conn) {
                    state.update(info, Some(PlayerSource::Bluez(path)));
                    std::thread::sleep(poll_interval);
                    continue;
                }
            }

            // Fall back to MPRIS players
            if let Some(conn) = &session_conn {
                if let Some((player_name, info)) = find_mpris_player(conn) {
                    state.update(info, Some(PlayerSource::Mpris(player_name)));
                    std::thread::sleep(poll_interval);
                    continue;
                }
            }

            // No active player
            state.update(MediaInfo::default(), None);
            std::thread::sleep(poll_interval);
        }
    });
}

/// Find an active BlueZ media player (Bluetooth A2DP)
fn find_bluez_player(conn: &Connection) -> Option<(String, MediaInfo)> {
    // Get all BlueZ objects
    let reply = conn.call_method(
        Some("org.bluez"),
        "/",
        Some("org.freedesktop.DBus.ObjectManager"),
        "GetManagedObjects",
        &(),
    ).ok()?;

    let body = reply.body();
    let objects: HashMap<zbus::zvariant::OwnedObjectPath, HashMap<String, HashMap<String, OwnedValue>>> =
        body.deserialize().ok()?;

    // Find MediaPlayer1 interfaces
    for (path, interfaces) in objects.iter() {
        if let Some(player_props) = interfaces.get("org.bluez.MediaPlayer1") {
            // Get status
            let status_val = player_props.get("Status")?;
            let status_str: String = status_val.try_clone().ok()?.try_into().ok()?;

            let status = match status_str.as_str() {
                "playing" => PlaybackStatus::Playing,
                "paused" => PlaybackStatus::Paused,
                _ => continue, // Skip stopped/error
            };

            // Get track info
            let track_val = player_props.get("Track")?;
            let track: HashMap<String, OwnedValue> = track_val.try_clone().ok()?.try_into().ok()?;

            let title: String = track.get("Title")
                .and_then(|v| v.try_clone().ok()?.try_into().ok())
                .unwrap_or_default();

            let artist: String = track.get("Artist")
                .and_then(|v| v.try_clone().ok()?.try_into().ok())
                .unwrap_or_default();

            let album: String = track.get("Album")
                .and_then(|v| v.try_clone().ok()?.try_into().ok())
                .unwrap_or_default();

            // Get player name from device
            let player_name: String = player_props.get("Name")
                .and_then(|v| v.try_clone().ok()?.try_into().ok())
                .unwrap_or_else(|| "Bluetooth".to_string());

            if !title.is_empty() {
                return Some((path.as_str().to_string(), MediaInfo {
                    title,
                    artist,
                    album,
                    status,
                    player_name,
                }));
            }
        }
    }

    None
}

/// Find an active MPRIS player and get its info
fn find_mpris_player(conn: &Connection) -> Option<(String, MediaInfo)> {
    // List all names on the bus
    let proxy = zbus::blocking::fdo::DBusProxy::new(conn).ok()?;
    let names = proxy.list_names().ok()?;

    // Find MPRIS players (org.mpris.MediaPlayer2.*)
    let mut best_player: Option<(String, MediaInfo)> = None;

    for name in names {
        let name_str = name.as_str();
        if !name_str.starts_with("org.mpris.MediaPlayer2.") {
            continue;
        }

        // Get player info
        if let Some(info) = get_player_info(conn, name_str) {
            // Prefer playing over paused
            match (&best_player, info.status) {
                (None, _) => {
                    best_player = Some((name_str.to_string(), info));
                }
                (Some((_, current)), PlaybackStatus::Playing)
                    if current.status != PlaybackStatus::Playing =>
                {
                    best_player = Some((name_str.to_string(), info));
                }
                _ => {}
            }
        }
    }

    best_player
}

/// Get media info from a specific player
fn get_player_info(conn: &Connection, player: &str) -> Option<MediaInfo> {
    // Get PlaybackStatus property
    let status_reply = conn
        .call_method(
            Some(player),
            "/org/mpris/MediaPlayer2",
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.mpris.MediaPlayer2.Player", "PlaybackStatus"),
        )
        .ok()?;

    let status_body = status_reply.body();
    let status_variant: Value = status_body.deserialize().ok()?;
    let status_str = extract_string_from_variant(&status_variant)?;
    let status = PlaybackStatus::from_str(&status_str);

    // Skip stopped players
    if status == PlaybackStatus::Stopped {
        return None;
    }

    // Get Metadata property
    let metadata_reply = conn
        .call_method(
            Some(player),
            "/org/mpris/MediaPlayer2",
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.mpris.MediaPlayer2.Player", "Metadata"),
        )
        .ok()?;

    let metadata_body = metadata_reply.body();
    let metadata_variant: Value = metadata_body.deserialize().ok()?;
    let metadata = extract_metadata(&metadata_variant);

    // Get player identity
    let identity_reply = conn
        .call_method(
            Some(player),
            "/org/mpris/MediaPlayer2",
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.mpris.MediaPlayer2", "Identity"),
        )
        .ok();

    let player_name = identity_reply
        .and_then(|r| {
            let body = r.body();
            let v: Value = body.deserialize().ok()?;
            extract_string_from_variant(&v)
        })
        .unwrap_or_else(|| {
            // Extract name from bus name (org.mpris.MediaPlayer2.playerName)
            player
                .strip_prefix("org.mpris.MediaPlayer2.")
                .unwrap_or(player)
                .to_string()
        });

    Some(MediaInfo {
        title: metadata.0,
        artist: metadata.1,
        album: metadata.2,
        status,
        player_name,
    })
}

/// Extract string from D-Bus variant (handles nested variants)
fn extract_string_from_variant(value: &Value) -> Option<String> {
    match value {
        Value::Str(s) => Some(s.to_string()),
        Value::Value(inner) => extract_string_from_variant(inner),
        _ => None,
    }
}

/// Extract metadata (title, artist, album) from D-Bus variant
fn extract_metadata(value: &Value) -> (String, String, String) {
    let empty = (String::new(), String::new(), String::new());

    // The metadata is a{sv} (dict of string to variant)
    let dict = match value {
        Value::Dict(d) => d,
        Value::Value(inner) => {
            if let Value::Dict(d) = inner.as_ref() {
                d
            } else {
                return empty;
            }
        }
        _ => return empty,
    };

    // Convert to HashMap for easier access
    let map: HashMap<String, &Value> = dict
        .iter()
        .filter_map(|(k, v)| {
            let key = match k {
                Value::Str(s) => s.to_string(),
                _ => return None,
            };
            Some((key, v))
        })
        .collect();

    let title = map
        .get("xesam:title")
        .and_then(|v| extract_string_from_variant(v))
        .unwrap_or_default();

    // Artist can be a string or array of strings
    let artist = map
        .get("xesam:artist")
        .and_then(|v| extract_artist(v))
        .unwrap_or_default();

    let album = map
        .get("xesam:album")
        .and_then(|v| extract_string_from_variant(v))
        .unwrap_or_default();

    (title, artist, album)
}

/// Extract artist (handles both string and array of strings)
fn extract_artist(value: &Value) -> Option<String> {
    match value {
        Value::Str(s) => Some(s.to_string()),
        Value::Array(arr) => {
            // Join multiple artists
            let artists: Vec<String> = arr
                .iter()
                .filter_map(|v| extract_string_from_variant(v))
                .collect();
            if artists.is_empty() {
                None
            } else {
                Some(artists.join(", "))
            }
        }
        Value::Value(inner) => extract_artist(inner),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_playback_status() {
        assert_eq!(PlaybackStatus::from_str("Playing"), PlaybackStatus::Playing);
        assert_eq!(PlaybackStatus::from_str("Paused"), PlaybackStatus::Paused);
        assert_eq!(PlaybackStatus::from_str("Stopped"), PlaybackStatus::Stopped);
        assert_eq!(PlaybackStatus::from_str("Unknown"), PlaybackStatus::Stopped);
    }

    #[test]
    fn test_media_info_active() {
        let info = MediaInfo {
            title: "Song".into(),
            status: PlaybackStatus::Playing,
            ..Default::default()
        };
        assert!(info.is_active());

        let stopped = MediaInfo::default();
        assert!(!stopped.is_active());
    }
}
