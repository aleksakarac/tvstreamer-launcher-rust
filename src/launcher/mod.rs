//! App launcher - process spawning with proper daemonization

use nix::libc;
use nix::sys::signal::{self, Signal};
use nix::unistd::{fork, setsid, ForkResult};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Arc;

/// Tracks launched app state
pub struct AppProcess {
    /// PID of launched process (0 if none)
    pid: AtomicI32,
    /// Whether an app is currently running
    pub running: AtomicBool,
}

impl AppProcess {
    pub fn new() -> Arc<Self> {
        // Ignore SIGCHLD to prevent zombies
        unsafe {
            signal::signal(Signal::SIGCHLD, signal::SigHandler::SigIgn).ok();
        }

        Arc::new(Self {
            pid: AtomicI32::new(0),
            running: AtomicBool::new(false),
        })
    }

    /// Launch an app command
    ///
    /// Returns true if launch succeeded
    pub fn launch(&self, command: &str) -> bool {
        // Already running?
        if self.running.load(Ordering::Acquire) {
            return false;
        }

        match unsafe { fork() } {
            Ok(ForkResult::Parent { child }) => {
                // Parent - store PID and mark as running
                self.pid.store(child.as_raw(), Ordering::Release);
                self.running.store(true, Ordering::Release);
                true
            }
            Ok(ForkResult::Child) => {
                // Child - create new session and exec
                let _ = setsid();

                // Execute command via shell
                let result = Command::new("/bin/sh").arg("-c").arg(command).status();

                // Exit child process
                std::process::exit(result.map(|s| s.code().unwrap_or(1)).unwrap_or(1));
            }
            Err(e) => {
                eprintln!("Fork failed: {e}");
                false
            }
        }
    }

    /// Check if the launched app is still running
    pub fn is_running(&self) -> bool {
        if !self.running.load(Ordering::Acquire) {
            return false;
        }

        let pid = self.pid.load(Ordering::Acquire);
        if pid <= 0 {
            return false;
        }

        // Check if process exists
        let exists = unsafe { libc::kill(pid, 0) == 0 };

        if !exists {
            // Process ended
            self.running.store(false, Ordering::Release);
            self.pid.store(0, Ordering::Release);
        }

        exists
    }

    /// Wait for app to exit (blocking)
    pub fn wait_for_exit(&self) {
        while self.is_running() {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    /// Get current PID if running
    pub fn get_pid(&self) -> Option<i32> {
        if self.running.load(Ordering::Acquire) {
            let pid = self.pid.load(Ordering::Acquire);
            if pid > 0 {
                return Some(pid);
            }
        }
        None
    }
}

/// System commands
pub fn reboot() -> std::io::Result<()> {
    Command::new("systemctl").arg("reboot").status()?;
    Ok(())
}

pub fn poweroff() -> std::io::Result<()> {
    Command::new("systemctl").arg("poweroff").status()?;
    Ok(())
}
