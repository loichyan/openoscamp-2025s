use core::fmt;
use log::{Level, LevelFilter, Metadata};

struct Logger {}

impl Logger {
    fn colored(&self, level: Level, contents: impl fmt::Display) -> impl fmt::Display {
        let color = match level {
            Level::Error => 31, // Red
            Level::Warn => 93,  // BrightYellow
            Level::Info => 34,  // Blue
            Level::Debug => 32, // Green
            Level::Trace => 90, // BrightBlack
        };
        fmt::from_fn(move |f| f.write_fmt(format_args!("\x1b[{color}m{contents}\x1b[0m")))
    }
}

impl log::Log for Logger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let level = record.level();
        println!(
            "[kernel] [{}] {}\x1b[0m",
            self.colored(level, level),
            record.args(),
        );
    }

    fn flush(&self) {}
}

/// # Safety
///
/// This function is thread-unsafe and may be called at most once.
pub unsafe fn init() {
    static LOGGER: Logger = Logger {};
    unsafe { log::set_logger_racy(&LOGGER).unwrap() };
    log::set_max_level(match option_env!("RUST_LOG") {
        Some("ERROR") => LevelFilter::Error,
        Some("WARN") => LevelFilter::Warn,
        Some("INFO") => LevelFilter::Info,
        Some("DEBUG") => LevelFilter::Debug,
        Some("TRACE") => LevelFilter::Trace,
        _ => LevelFilter::Debug,
    });
}
