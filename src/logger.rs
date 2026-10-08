use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

const LOG_FILENAME: &str = "flow8-midi.log";
const RING_BUFFER_CAPACITY: usize = 2000;
/// The log file rolls over to .old.log past this size.
const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

/// DEBUG lines (one per SysEx value, BLE scan details) go to the in-app
/// buffer only -- Export Log still has them -- unless FLOW8_DEBUG=1, which
/// also writes them to the file and stderr. Writing them always grew the
/// log by ~330 MB a day while the mixer was connected.
fn debug_to_file() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("FLOW8_DEBUG").map_or(false, |v| v == "1"))
}

/// Per-user log folder, never next to the executable (which may be
/// /usr/local/bin or Program Files).
fn log_dir() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let dir = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        home.map(|h| h.join("Library").join("Logs"))
    } else {
        std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| home.map(|h| h.join(".local").join("state")))
    };
    dir.unwrap_or_else(std::env::temp_dir).join("flow-8-midi")
}

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static LOG_BUFFER: OnceLock<Mutex<VecDeque<LogEntry>>> = OnceLock::new();

#[derive(Debug, Clone)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Debug => write!(f, "DEBUG"),
            LogLevel::Info => write!(f, "INFO"),
            LogLevel::Warn => write!(f, "WARN"),
            LogLevel::Error => write!(f, "ERROR"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: LogLevel,
    pub message: String,
}

impl std::fmt::Display for LogEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] [{}] {}", self.timestamp, self.level, self.message)
    }
}

fn timestamp() -> String {
    chrono::Local::now().format("%H:%M:%S%.3f").to_string()
}

fn system_diagnostics() -> String {
    format!(
        "OS: {} {} ({})\nHostname: {}\nApp: FLOW 8 MIDI Controller v{}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::env::consts::FAMILY,
        hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string()),
        env!("CARGO_PKG_VERSION"),
    )
}

pub fn init() {
    LOG_BUFFER
        .set(Mutex::new(VecDeque::with_capacity(RING_BUFFER_CAPACITY)))
        .ok();

    let dir = log_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(LOG_FILENAME);

    let header = format!(
        "=== FLOW 8 MIDI Controller v{} ===\n{}",
        env!("CARGO_PKG_VERSION"),
        system_diagnostics()
    );

    // Keep the previous run's log: a crash's evidence would otherwise be
    // wiped by the restart that follows it.
    let _ = std::fs::rename(&path, path.with_extension("prev.log"));

    if let Ok(mut file) = OpenOptions::new()
        .write(true)
        .truncate(true)
        .create(true)
        .open(&path)
    {
        let _ = writeln!(file, "[{}] {}", timestamp(), header);
    }

    LOG_PATH.set(path).ok();

    log_with_level(LogLevel::Info, &header);

    // Panics go to stderr, which is lost when started from a menu: write
    // them (with a backtrace) to the log too.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        log_with_level(LogLevel::Error, &format!(
            "PANIC in thread '{}': {}\n{}",
            thread.name().unwrap_or("?"), info,
            std::backtrace::Backtrace::force_capture()));
        default_hook(info);
    }));
}

fn push_to_buffer(entry: LogEntry) {
    if let Some(buffer) = LOG_BUFFER.get() {
        if let Ok(mut buf) = buffer.lock() {
            if buf.len() >= RING_BUFFER_CAPACITY {
                buf.pop_front();
            }
            buf.push_back(entry);
        }
    }
}

pub fn log_with_level(level: LogLevel, message: &str) {
    let ts = timestamp();
    let entry = LogEntry {
        timestamp: ts.clone(),
        level: level.clone(),
        message: message.to_string(),
    };

    if matches!(level, LogLevel::Debug) && !debug_to_file() {
        push_to_buffer(entry);
        return;
    }

    eprintln!("{}", entry);
    push_to_buffer(entry);

    if let Some(path) = LOG_PATH.get() {
        if std::fs::metadata(path).map_or(false, |m| m.len() > MAX_LOG_BYTES) {
            let _ = std::fs::rename(path, path.with_extension("old.log"));
        }
        if let Ok(mut file) = OpenOptions::new().append(true).create(true).open(path) {
            let _ = writeln!(file, "[{}] [{}] {}", ts, level, message);
        }
    }
}

pub fn log(message: &str) {
    log_with_level(LogLevel::Info, message);
}

pub fn export_log() -> String {
    let mut output = String::new();
    output.push_str(&format!("--- Debug Report ---\n{}\n\n", system_diagnostics()));

    if let Some(buffer) = LOG_BUFFER.get() {
        if let Ok(buf) = buffer.lock() {
            for entry in buf.iter() {
                output.push_str(&format!("{}\n", entry));
            }
        }
    }

    output.push_str("--- End of Report ---\n");
    output
}

pub fn get_recent_entries(count: usize) -> Vec<LogEntry> {
    if let Some(buffer) = LOG_BUFFER.get() {
        if let Ok(buf) = buffer.lock() {
            return buf.iter().rev().take(count).cloned().collect::<Vec<_>>().into_iter().rev().collect();
        }
    }
    vec![]
}

pub fn entry_count() -> usize {
    if let Some(buffer) = LOG_BUFFER.get() {
        if let Ok(buf) = buffer.lock() {
            return buf.len();
        }
    }
    0
}

#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => {
        $crate::logger::log(&format!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => {
        $crate::logger::log_with_level($crate::logger::LogLevel::Debug, &format!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => {
        $crate::logger::log_with_level($crate::logger::LogLevel::Warn, &format!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        $crate::logger::log_with_level($crate::logger::LogLevel::Error, &format!($($arg)*))
    };
}
