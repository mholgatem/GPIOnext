/// Timestamped logger matching the Python daemon's output format:
///   `2025-01-15 02:30:45PM SYSTEM - message`
///
/// Controlled by two flags:
/// - `dev`   — write to stdout
/// - `debug` — write to `/opt/gpionext/logFile.txt`
///
/// Both flags can be active simultaneously.

use std::{
    fs::{File, OpenOptions},
    io::Write,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

pub struct Logger {
    dev: bool,
    file: Option<Mutex<File>>,
}

impl Logger {
    pub fn new(dev: bool, debug: bool) -> Self {
        let file = if debug {
            let path = "/opt/gpionext/logFile.txt";
            match OpenOptions::new().create(true).write(true).truncate(true).open(path) {
                Ok(f) => Some(Mutex::new(f)),
                Err(e) => {
                    eprintln!("[gpionext] WARNING: cannot open log file {path}: {e}");
                    None
                }
            }
        } else {
            None
        };
        Self { dev, file }
    }

    pub fn log(&self, msg: &str) {
        if !self.dev && self.file.is_none() {
            return;
        }
        let ts = timestamp();
        let line = format!("{ts} SYSTEM - {msg}\n");
        if self.dev {
            print!("{line}");
        }
        if let Some(f) = &self.file {
            if let Ok(mut guard) = f.lock() {
                let _ = guard.write_all(line.as_bytes());
            }
        }
    }
}

fn timestamp() -> String {
    // Build a simple timestamp without pulling in a date library.
    // Format: YYYY-MM-DD HH:MM:SSam/pm  (matches Python daemon's datetime format)
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Days since Unix epoch → approximate calendar date (no DST / timezone adjustment)
    let s = secs % 86400;
    let days = secs / 86400;

    let h = (s / 3600) as u32;
    let m = ((s % 3600) / 60) as u32;
    let sc = (s % 60) as u32;

    let h12 = if h % 12 == 0 { 12 } else { h % 12 };
    let ampm = if h < 12 { "AM" } else { "PM" };

    // Gregorian calendar reconstruction from day count
    let (year, month, day) = days_to_ymd(days);

    format!("{year:04}-{month:02}-{day:02} {h12:02}:{m:02}:{sc:02}{ampm}")
}

fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {
    // Shift from Unix epoch (1970-01-01) using the 400-year Gregorian cycle
    days += 719468; // offset to 0000-03-01
    let era = days / 146097;
    let doe = days % 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}
