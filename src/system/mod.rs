//! System monitoring - optimized for minimal allocations
//!
//! Reads from /proc and /sys with reused buffers

use nix::libc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Atomic stats container - lock-free updates
pub struct SystemStats {
    pub cpu: AtomicI32,
    pub ram: AtomicI32,
    pub temp: AtomicI32,
    pub disk: AtomicI32,
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

    /// Get current values
    pub fn get(&self) -> (i32, i32, i32, i32) {
        (
            self.cpu.load(Ordering::Relaxed),
            self.ram.load(Ordering::Relaxed),
            self.temp.load(Ordering::Relaxed),
            self.disk.load(Ordering::Relaxed),
        )
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

/// Start background stats thread
pub fn start_stats_thread(stats: Arc<SystemStats>, interval_ms: u64) {
    std::thread::spawn(move || {
        let mut cpu_state = CpuState::new();
        // Reuse buffer for file reads
        let mut buffer = String::with_capacity(4096);

        while stats.running.load(Ordering::Acquire) {
            let new_cpu = read_cpu(&mut buffer, &mut cpu_state);
            let new_ram = read_ram(&mut buffer);
            let new_temp = read_temp(&mut buffer);
            let new_disk = read_disk();

            // Check if anything changed
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stats_creation() {
        let stats = SystemStats::new();
        assert_eq!(stats.cpu.load(Ordering::Relaxed), 0);
    }
}
