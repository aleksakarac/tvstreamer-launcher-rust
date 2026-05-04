# tvstreamer-launcher (Rust)

**Lightweight media center launcher for Raspberry Pi — built with Rust and Slint.**

A minimal, resource-optimized app launcher designed to run on low-power hardware as a living room media center frontend. Launches Kodi, Stremio, IPTV, and other apps from a clean fullscreen grid UI with live system stats and a screensaver.

![Rust](https://img.shields.io/badge/Rust-000000?style=flat-square&logo=rust)
![Slint](https://img.shields.io/badge/Slint_UI-2379F4?style=flat-square)
![Raspberry Pi](https://img.shields.io/badge/Raspberry_Pi-A22846?style=flat-square&logo=raspberrypi&logoColor=white)
![License](https://img.shields.io/badge/license-MIT-green?style=flat-square)

---

## Features

- **App Grid** — Fullscreen launcher grid with Nerd Font glyph icons, JSON-configurable
- **System Stats Bar** — Live CPU, RAM, temperature, disk, network, Bluetooth — updated every 2s
- **Now Playing** — MPRIS D-Bus integration shows current media title and artist
- **Bluetooth Status** — BlueZ D-Bus integration for connected device display
- **Screensaver** — Activates after configurable idle timeout, shows clock + now playing
- **Arc Blueberry Theme** — Dark theme matching the Arc GTK color scheme
- **`hide_launcher` mode** — Hides the launcher while an app is running, restores on exit
- **JSON Config** — Apps, stats, display, screensaver all configured via `config/default.json`

## Resource Usage

Binary is aggressively size-optimized for Raspberry Pi:

```toml
[profile.release]
opt-level = "z"   # optimize for size
lto = true
codegen-units = 1
panic = "abort"
strip = true
```

## Getting Started

**Prerequisites:** Rust 1.70+, a Wayland or X11 compositor

```bash
git clone https://github.com/aleksakarac/tvstreamer-launcher-rust
cd tvstreamer-launcher-rust

# Development
cargo run

# Release build (size-optimized)
cargo build --release
```

The binary reads `config/default.json` on startup. Copy and edit it to configure your apps.

## Configuration

```json
{
  "apps": [
    {
      "name": "Kodi",
      "command": "kodi",
      "icon": "󰕼",
      "icon_type": "glyph",
      "hide_launcher": true
    }
  ],
  "stats": { "show_cpu": true, "show_temp": true, "update_interval_ms": 2000 },
  "display": { "fullscreen": true, "grid_columns": 4 },
  "screensaver": { "enabled": true, "timeout_seconds": 300, "show_now_playing": true }
}
```

## Tech Stack

| | |
|---|---|
| Language | Rust (edition 2024) |
| UI Framework | [Slint](https://slint.dev) 1.9 |
| Async Runtime | Tokio |
| D-Bus (MPRIS/BlueZ) | zbus |
| Config | serde + serde_json |

## Related

- [`tvstreamer-launcher`](https://github.com/aleksakarac/tvstreamer-launcher) — Python/Pygame version
- [`tvstreamer-launcher-c`](https://github.com/aleksakarac/tvstreamer-launcher-c) — C/SDL2 version (smallest binary)
