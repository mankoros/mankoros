// Copyright (c) 2023 Easton Man
// Copyright (c) 2020 rCore
//
// Adapted from rCore https://github.com/rcore-os/rCore/blob/13ad2d19058901e6401a978d4e20acf7f5610666/kernel/src/logging.rs

use core::fmt;

use log::{self, Level, LevelFilter, Log, Metadata, Record};

// Currently only error, warn, info and debug is used
// Any lower level is ignored
const LOG_LEVEL: &str = "info";

pub fn init() {
    static LOGGER: SimpleLogger = SimpleLogger;
    log::set_logger(&LOGGER).unwrap();
    log::set_max_level(match LOG_LEVEL {
        "error" => LevelFilter::Error,
        "warn" => LevelFilter::Warn,
        "info" => LevelFilter::Info,
        "debug" => LevelFilter::Debug,
        _ => LevelFilter::Off,
    })
}

struct SimpleLogger;

impl Log for SimpleLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }
    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        print_in_color(
            format_args!("[{:>5}] {}\n", record.level(), record.args()),
            level_to_color_code(record.level()),
        );
    }
    fn flush(&self) {}
}

/// Add escape sequence to print with color in Linux console
macro_rules! with_color {
    ($args: ident, $color_code: ident) => {{
        format_args!("\u{1B}[{}m{}\u{1B}[0m", $color_code as u8, $args)
    }};
}

fn print_in_color(args: fmt::Arguments, color_code: u8) {
    crate::print!("{}", with_color!(args, color_code));
}

fn level_to_color_code(level: Level) -> u8 {
    match level {
        Level::Error => 93, // BrightYellow
        Level::Warn => 34,  // Blue
        Level::Info => 0,   // Normal
        Level::Debug => 0,  // Normal
        Level::Trace => 0,  // Normal
    }
}
