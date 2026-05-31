//! Simple `log` backend (no extra crates).

use log::{LevelFilter, Log, Metadata, Record};
use std::sync::OnceLock;

struct SimpleLogger;

static LOGGER: SimpleLogger = SimpleLogger;
static MAX_LEVEL: OnceLock<LevelFilter> = OnceLock::new();

impl Log for SimpleLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= max_level()
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            eprintln!("[{}] {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

fn max_level() -> LevelFilter {
    *MAX_LEVEL.get_or_init(|| {
        std::env::var("AEVIA_LOG")
            .ok()
            .and_then(|v| match v.to_lowercase().as_str() {
                "trace" => Some(LevelFilter::Trace),
                "debug" => Some(LevelFilter::Debug),
                "info" => Some(LevelFilter::Info),
                "warn" => Some(LevelFilter::Warn),
                "error" => Some(LevelFilter::Error),
                "off" => Some(LevelFilter::Off),
                _ => None,
            })
            .unwrap_or(LevelFilter::Info)
    })
}

/// Install the global logger. Safe to call once per process.
pub fn init() {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(max_level());
    log::debug!(target: "aevia", "logging initialized at {}", max_level());
}

/// Log the start of a compiler pass.
pub fn pass(name: &str) {
    log::info!(target: "aevia::pass", "{}", name);
}

#[allow(dead_code)]
pub fn pass_detail(name: &str, detail: &str) {
    log::info!(target: "aevia::pass", "{}: {}", name, detail);
}
