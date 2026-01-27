//! TvStreamer Launcher - Lightweight media center UI
//!
//! Optimized for minimal resource usage on Raspberry Pi

mod config;
mod launcher;
mod system;
mod ui;

use config::Config;
use launcher::AppProcess;
use slint::ComponentHandle;
use std::sync::Arc;
use std::time::Duration;

fn main() {
    // Load configuration
    let config = Config::load();

    // Create main window
    let window = ui::MainWindow::new().expect("Failed to create window");

    // Set up apps
    let apps_model = ui::apps_to_slint(&config);
    window.set_apps(apps_model);
    window.set_grid_columns(config.display.grid_columns as i32);

    // Initial time
    window.set_time(ui::format_time());
    window.set_date(ui::format_date());

    // Start system stats thread
    let stats = system::SystemStats::new();
    system::start_stats_thread(Arc::clone(&stats), config.stats.update_interval_ms);

    // App process tracker
    let app_process = AppProcess::new();

    // Store config for callbacks
    let config_for_launch = Arc::new(config);

    // Set up callbacks
    {
        let config = Arc::clone(&config_for_launch);
        let process = Arc::clone(&app_process);
        let window_weak = window.as_weak();

        window.on_app_activated(move |index| {
            let idx = index as usize;
            if idx >= config.apps.len() {
                return;
            }

            let app = &config.apps[idx];
            println!("Launching: {}", app.command);

            if app.hide_launcher {
                // Hide window before launching
                if let Some(w) = window_weak.upgrade() {
                    w.window().set_minimized(true);
                }
            }

            // Launch the app
            if process.launch(&app.command) {
                // Wait for app to exit in background
                let process_clone = Arc::clone(&process);
                let window_weak_clone = window_weak.clone();
                let hide = app.hide_launcher;

                std::thread::spawn(move || {
                    process_clone.wait_for_exit();

                    if hide {
                        // Show window again
                        slint::invoke_from_event_loop(move || {
                            if let Some(w) = window_weak_clone.upgrade() {
                                w.window().set_minimized(false);
                            }
                        })
                        .ok();
                    }
                });
            }
        });
    }

    // Quit callback
    {
        let window_weak = window.as_weak();
        window.on_request_quit(move || {
            if let Some(w) = window_weak.upgrade() {
                w.window().hide().ok();
            }
            slint::quit_event_loop().ok();
        });
    }

    // Settings callback
    window.on_request_settings(|| {
        println!("Settings requested");
        // TODO: Show settings screen
    });

    // Key press callback
    window.on_key_pressed(|key| {
        let key_str: String = key.into();
        match key_str.as_str() {
            "r" | "R" => {
                println!("Reboot requested");
                // TODO: Show confirmation dialog
            }
            "p" | "P" => {
                println!("Poweroff requested");
                // TODO: Show confirmation dialog
            }
            _ => {}
        }
    });

    // Track last minute for clock updates
    let mut last_minute = ui::current_minute();

    // Clone stats for timer closure
    let stats_for_timer = Arc::clone(&stats);

    // Main event loop with timer for updates
    let window_weak = window.as_weak();
    let timer = slint::Timer::default();

    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(100),
        move || {
            let Some(window) = window_weak.upgrade() else {
                return;
            };

            // Check if minute changed (for clock)
            let current_minute = ui::current_minute();
            if current_minute != last_minute {
                last_minute = current_minute;
                window.set_time(ui::format_time());
                window.set_date(ui::format_date());
            }

            // Check if stats changed
            if stats_for_timer.take_changed() {
                let (cpu, ram, temp, disk) = stats_for_timer.get();
                window.set_cpu(cpu);
                window.set_ram(ram);
                window.set_temp(temp);
                window.set_disk(disk);
            }
        },
    );

    // Run the event loop
    window.run().expect("Failed to run window");

    // Cleanup
    stats.stop();
}
