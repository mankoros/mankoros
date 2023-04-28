// Copyright (c) 2023 Easton Man
// Copyright (c) 2020 rCore
// Copyright (c) 2023 rCore/AcreOS
//
// Adapted from rCore https://github.com/rcore-os/rCore/blob/13ad2d19058901e6401a978d4e20acf7f5610666/kernel/src/logging.rs
// And AcreOS/modules/axlog/src/lib.rs

use core::fmt;
use core::str::FromStr;

use log::{self, Level, LevelFilter, Log, Metadata, Record};

macro_rules! with_color {
    ($color_code:expr, $($arg:tt)*) => {{
        format_args!("\u{1B}[{}m{}\u{1B}[m", $color_code as u8, format_args!($($arg)*))
    }};
}

#[repr(u8)]
#[allow(dead_code)]
enum ColorCode {
    Black = 30,
    Red = 31,
    Green = 32,
    Yellow = 33,
    Blue = 34,
    Magenta = 35,
    Cyan = 36,
    White = 37,
    BrightBlack = 90,
    BrightRed = 91,
    BrightGreen = 92,
    BrightYellow = 93,
    BrightBlue = 94,
    BrightMagenta = 95,
    BrightCyan = 96,
    BrightWhite = 97,
}

fn __print_impl(args: fmt::Arguments) {
    crate::print!("{}", args);
}

// Currently only error, warn, info and debug is used
// Any lower level is ignored
const LOG_LEVEL: &str = "debug";

pub fn init() {
    static LOGGER: SimpleLogger = SimpleLogger;
    log::set_logger(&LOGGER).unwrap();
    set_max_level(LOG_LEVEL);
}

pub fn set_max_level(level: &str) {
    let lf = LevelFilter::from_str(level).ok().unwrap_or(LevelFilter::Off);
    log::set_max_level(lf);
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
        let level = record.level();
        let _line = record.line().unwrap_or(0);
        let target = record.file().unwrap_or("");
        let level_color = match level {
            Level::Error => ColorCode::BrightRed,
            Level::Warn => ColorCode::BrightYellow,
            Level::Info => ColorCode::BrightGreen,
            Level::Debug => ColorCode::BrightCyan,
            Level::Trace => ColorCode::BrightBlack,
        };
        let args_color = match level {
            Level::Error => ColorCode::Red,
            Level::Warn => ColorCode::Yellow,
            Level::Info => ColorCode::Green,
            Level::Debug => ColorCode::Cyan,
            Level::Trace => ColorCode::BrightBlack,
        };
        __print_impl(with_color!(
            ColorCode::White,
            "[{} {} {} {}\n",
            with_color!(level_color, "{:<5}", level),
            with_color!(ColorCode::BrightBlue, "{:0>4}", crate::ticks()),
            with_color!(ColorCode::White, "{:<25}]", target),
            with_color!(args_color, "{}", record.args()),
        ));
    }
    fn flush(&self) {}
}
