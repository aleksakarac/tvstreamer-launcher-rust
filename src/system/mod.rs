//! System monitoring - optimized for minimal allocations
//!
//! Reads from /proc and /sys with reused buffers

use nix::libc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// Atomic stats container - lock-free updates for integers, RwLock for strings
pub struct SystemStats {
    pub cpu: AtomicI32,
    pub ram: AtomicI32,
    pub temp: AtomicI32,
    pub disk: AtomicI32,
    pub network: RwLock<String>,
    pub bluetooth: RwLock<String>,
    pub changed: AtomicBool,
    running: AtomicBool,
}

impl SystemStats {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            cpu: AtomicI32::new(0),
            ram: AtomicI32::new(0),
            temp: AtomicI32::new(0),
            disk: AtomicI32::new(0),
            network: RwLock::new("---".to_string()),
            bluetooth: RwLock::new("---".to_string()),
            changed: AtomicBool::new(false),
            running: AtomicBool::new(true),
        })
    }

    /// Stop the stats thread
    pub fn stop(&self) {
        self.running.store(false, Ordering::Release);
    }

    /// Check if stats changed and reset flag
    pub fn take_changed(&self) -> bool {
        self.changed.swap(false, Ordering::AcqRel)
    }

    /// Get current integer values
    pub fn get(&self) -> (i32, i32, i32, i32) {
        (
            self.cpu.load(Ordering::Relaxed),
            self.ram.load(Ordering::Relaxed),
            self.temp.load(Ordering::Relaxed),
            self.disk.load(Ordering::Relaxed),
        )
    }

    /// Get network status string
    pub fn get_network(&self) -> String {
        self.network.read().unwrap().clone()
    }

    /// Get bluetooth status string
    pub fn get_bluetooth(&self) -> String {
        self.bluetooth.read().unwrap().clone()
    }
}

/// CPU state for delta calculation
struct CpuState {
    prev_total: u64,
    prev_idle: u64,
}

impl CpuState {
    fn new() -> Self {
        Self {
            prev_total: 0,
            prev_idle: 0,
        }
    }
}

/// Network/Bluetooth poll counter (poll less frequently than CPU/RAM)
struct SlowPollState {
    counter: u64,
    network_cache: String,
    bluetooth_cache: String,
}

impl SlowPollState {
    fn new() -> Self {
        Self {
            counter: 0,
            network_cache: "---".to_string(),
            bluetooth_cache: "---".to_string(),
        }
    }
}

/// Start background stats thread
pub fn start_stats_thread(stats: Arc<SystemStats>, interval_ms: u64) {
    std::thread::spawn(move || {
        let mut cpu_state = CpuState::new();
        let mut slow_poll = SlowPollState::new();
        // Reuse buffer for file reads
        let mut buffer = String::with_capacity(4096);

        // Calculate how many iterations = ~3 seconds for network/bluetooth poll
        let slow_poll_interval = (3000 / interval_ms).max(1);

        // Create tokio runtime for async bluetooth operations
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok();

        while stats.running.load(Ordering::Acquire) {
            let new_cpu = read_cpu(&mut buffer, &mut cpu_state);
            let new_ram = read_ram(&mut buffer);
            let new_temp = read_temp(&mut buffer);
            let new_disk = read_disk();

            // Poll network/bluetooth less frequently
            slow_poll.counter += 1;
            if slow_poll.counter >= slow_poll_interval {
                slow_poll.counter = 0;

                // Read network status (synchronous)
                let new_network = read_network(&mut buffer);

                // Read bluetooth status (async via tokio)
                let new_bluetooth = if let Some(ref rt) = rt {
                    rt.block_on(read_bluetooth())
                } else {
                    "---".to_string()
                };

                // Update caches if changed
                if new_network != slow_poll.network_cache {
                    slow_poll.network_cache = new_network;
                    if let Ok(mut lock) = stats.network.write() {
                        *lock = slow_poll.network_cache.clone();
                    }
                    stats.changed.store(true, Ordering::Release);
                }

                if new_bluetooth != slow_poll.bluetooth_cache {
                    slow_poll.bluetooth_cache = new_bluetooth;
                    if let Ok(mut lock) = stats.bluetooth.write() {
                        *lock = slow_poll.bluetooth_cache.clone();
                    }
                    stats.changed.store(true, Ordering::Release);
                }
            }

            // Check if integer stats changed
            let old_cpu = stats.cpu.load(Ordering::Relaxed);
            let old_ram = stats.ram.load(Ordering::Relaxed);
            let old_temp = stats.temp.load(Ordering::Relaxed);
            let old_disk = stats.disk.load(Ordering::Relaxed);

            if new_cpu != old_cpu
                || new_ram != old_ram
                || new_temp != old_temp
                || new_disk != old_disk
            {
                stats.cpu.store(new_cpu, Ordering::Relaxed);
                stats.ram.store(new_ram, Ordering::Relaxed);
                stats.temp.store(new_temp, Ordering::Relaxed);
                stats.disk.store(new_disk, Ordering::Relaxed);
                stats.changed.store(true, Ordering::Release);
            }

            std::thread::sleep(Duration::from_millis(interval_ms));
        }
    });
}

/// Read CPU usage from /proc/stat
fn read_cpu(buffer: &mut String, state: &mut CpuState) -> i32 {
    buffer.clear();

    if std::fs::read_to_string("/proc/stat")
        .map(|s| {
            buffer.push_str(&s);
        })
        .is_err()
    {
        return 0;
    }

    // Parse first line: cpu user nice system idle iowait irq softirq
    let line = match buffer.lines().next() {
        Some(l) => l,
        None => return 0,
    };

    let mut parts = line.split_whitespace().skip(1); // Skip "cpu"

    let user: u64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let nice: u64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let system: u64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let idle: u64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let iowait: u64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let irq: u64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let softirq: u64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);

    let total = user + nice + system + idle + iowait + irq + softirq;
    let idle_total = idle + iowait;

    // Calculate delta
    let total_delta = total.saturating_sub(state.prev_total);
    let idle_delta = idle_total.saturating_sub(state.prev_idle);

    state.prev_total = total;
    state.prev_idle = idle_total;

    if total_delta == 0 {
        return 0;
    }

    let usage = ((total_delta - idle_delta) * 100) / total_delta;
    usage as i32
}

/// Read RAM usage from /proc/meminfo
fn read_ram(buffer: &mut String) -> i32 {
    buffer.clear();

    if std::fs::read_to_string("/proc/meminfo")
        .map(|s| {
            buffer.push_str(&s);
        })
        .is_err()
    {
        return 0;
    }

    let mut total: u64 = 0;
    let mut available: u64 = 0;

    for line in buffer.lines() {
        if line.starts_with("MemTotal:") {
            total = parse_meminfo_value(line);
        } else if line.starts_with("MemAvailable:") {
            available = parse_meminfo_value(line);
        }

        if total > 0 && available > 0 {
            break;
        }
    }

    if total == 0 {
        return 0;
    }

    let used = total.saturating_sub(available);
    ((used * 100) / total) as i32
}

/// Parse memory value from line like "MemTotal:        8000000 kB"
fn parse_meminfo_value(line: &str) -> u64 {
    line.split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// Read temperature from thermal zone
fn read_temp(buffer: &mut String) -> i32 {
    buffer.clear();

    // Try multiple thermal zones
    let paths = [
        "/sys/class/thermal/thermal_zone0/temp",
        "/sys/class/thermal/thermal_zone1/temp",
    ];

    for path in paths {
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(millidegrees) = content.trim().parse::<i32>() {
                return millidegrees / 1000;
            }
        }
    }

    0
}

/// Read disk usage
fn read_disk() -> i32 {
    use std::ffi::CString;
    use std::mem::MaybeUninit;

    let path = CString::new("/").unwrap();
    let mut stat: MaybeUninit<libc::statvfs> = MaybeUninit::uninit();

    unsafe {
        if libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) != 0 {
            return 0;
        }

        let stat = stat.assume_init();
        let total = stat.f_blocks * stat.f_frsize as u64;
        let free = stat.f_bfree * stat.f_frsize as u64;

        if total == 0 {
            return 0;
        }

        let used = total - free;
        ((used * 100) / total) as i32
    }
}

/// Read network status - returns SSID for WiFi or "Ethernet" for wired
fn read_network(buffer: &mut String) -> String {
    // Check common interface names for connectivity
    let interfaces = ["wlan0", "eth0", "end0", "enp0s3", "eno1"];

    for iface in interfaces {
        let operstate_path = format!("/sys/class/net/{}/operstate", iface);

        // Check if interface is up
        if let Ok(state) = std::fs::read_to_string(&operstate_path) {
            if state.trim() != "up" {
                continue;
            }

            // Interface is up - check if it's wireless or wired
            let wireless_path = format!("/sys/class/net/{}/wireless", iface);
            if std::path::Path::new(&wireless_path).exists() {
                // It's a wireless interface - try to get SSID
                if let Some(ssid) = get_wifi_ssid(iface, buffer) {
                    return ssid;
                }
                return "WiFi".to_string();
            } else {
                // It's a wired interface
                return "Ethernet".to_string();
            }
        }
    }

    "---".to_string()
}

/// Get WiFi SSID using iwgetid command
fn get_wifi_ssid(interface: &str, _buffer: &mut String) -> Option<String> {
    // Try iwgetid first (most reliable)
    if let Ok(output) = std::process::Command::new("iwgetid")
        .arg(interface)
        .arg("-r") // Raw SSID output
        .output()
    {
        if output.status.success() {
            let ssid = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !ssid.is_empty() {
                // Truncate long SSIDs for display
                if ssid.len() > 12 {
                    return Some(format!("{}...", &ssid[..9]));
                }
                return Some(ssid);
            }
        }
    }

    // Fallback: try reading from /proc/net/wireless
    // This only tells us if we're connected, not the SSID
    None
}

/// Read Bluetooth status via D-Bus (BlueZ)
async fn read_bluetooth() -> String {
    match get_connected_bluetooth_device().await {
        Ok(Some(name)) => {
            // Truncate long device names for display
            if name.len() > 12 {
                format!("{}...", &name[..9])
            } else {
                name
            }
        }
        Ok(None) => "---".to_string(),
        Err(_) => "---".to_string(),
    }
}

/// Query BlueZ over D-Bus for connected devices
async fn get_connected_bluetooth_device() -> Result<Option<String>, zbus::Error> {
    use zbus::zvariant::Value;

    // Connect to system bus
    let connection = zbus::Connection::system().await?;

    // Get ObjectManager interface for BlueZ
    let proxy = zbus::fdo::ObjectManagerProxy::builder(&connection)
        .destination("org.bluez")?
        .path("/")?
        .build()
        .await?;

    // Get all managed objects
    let objects = proxy.get_managed_objects().await?;

    // Look for connected devices
    for (_path, interfaces) in objects {
        // Check if this is a device (has org.bluez.Device1 interface)
        if let Some(device_props) = interfaces.get("org.bluez.Device1") {
            // Check if device is connected
            if let Some(connected) = device_props.get("Connected") {
                let is_connected = match connected.downcast_ref::<Value>() {
                    Ok(Value::Bool(b)) => b,
                    _ => {
                        // Try direct bool access
                        match connected.downcast_ref::<bool>() {
                            Ok(b) => b,
                            _ => false,
                        }
                    }
                };

                if is_connected {
                    // Get device name
                    if let Some(name_value) = device_props.get("Name") {
                        if let Ok(Value::Str(name)) = name_value.downcast_ref::<Value>() {
                            return Ok(Some(name.to_string()));
                        }
                    }
                    // Fallback to alias
                    if let Some(alias_value) = device_props.get("Alias") {
                        if let Ok(Value::Str(alias)) = alias_value.downcast_ref::<Value>() {
                            return Ok(Some(alias.to_string()));
                        }
                    }
                    // Found connected device but no name
                    return Ok(Some("BT Device".to_string()));
                }
            }
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stats_creation() {
        let stats = SystemStats::new();
        assert_eq!(stats.cpu.load(Ordering::Relaxed), 0);
    }
}
