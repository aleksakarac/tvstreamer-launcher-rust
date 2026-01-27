//! UI bridge - connects Rust backend to Slint frontend

use crate::config::Config;

// Include generated Slint code
slint::include_modules!();

/// Convert config apps to Slint AppInfo
pub fn apps_to_slint(config: &Config) -> slint::ModelRc<AppInfo> {
    let apps: Vec<AppInfo> = config
        .apps
        .iter()
        .map(|app| AppInfo {
            name: app.name.clone().into(),
            icon: app.icon.clone().into(),
            command: app.command.clone().into(),
        })
        .collect();

    slint::ModelRc::new(slint::VecModel::from(apps))
}

/// Format time string (HH:MM)
pub fn format_time() -> slint::SharedString {
    use std::time::SystemTime;

    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Calculate local time (simplified - assumes UTC for now)
    // In production, use chrono crate for proper timezone handling
    let hours = ((now / 3600) % 24) as u32;
    let minutes = ((now / 60) % 60) as u32;

    // 12-hour format
    let display_hours = if hours == 0 {
        12
    } else if hours > 12 {
        hours - 12
    } else {
        hours
    };

    // Pre-allocate exact size needed
    let mut s = String::with_capacity(5);
    use std::fmt::Write;
    write!(s, "{:02}:{:02}", display_hours, minutes).unwrap();
    s.into()
}

/// Format date string
pub fn format_date() -> slint::SharedString {
    use std::time::SystemTime;

    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Days since Unix epoch
    let days = now / 86400;

    // Day of week (0 = Thursday for Unix epoch)
    let dow = ((days + 4) % 7) as usize;
    let day_names = [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ];

    // Calculate date (simplified algorithm)
    let (_year, month, day) = days_to_ymd(days as i64);
    let month_names = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];

    format!(
        "{}, {} {}",
        day_names[dow],
        month_names[(month - 1) as usize],
        day
    )
    .into()
}

/// Convert days since epoch to year/month/day
fn days_to_ymd(days: i64) -> (i32, u32, u32) {
    // Algorithm from Howard Hinnant
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    (y as i32, m, d)
}

/// Get current minute (for change detection)
pub fn current_minute() -> u32 {
    use std::time::SystemTime;

    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    ((now / 60) % (24 * 60)) as u32
}
