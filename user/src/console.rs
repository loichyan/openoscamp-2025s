use core::fmt;
use core::fmt::Write;

struct Stdout;

impl Write for Stdout {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        write_bytes(s.as_bytes())
    }
}

pub fn write_bytes(bytes: &[u8]) -> fmt::Result {
    if crate::syscall::sys_write(common::syscall::FD_STDOUT, bytes) < 0 {
        Err(fmt::Error)
    } else {
        Ok(())
    }
}

pub fn print_silent(args: fmt::Arguments) {
    Stdout.write_fmt(args).ok();
}

pub fn print(args: fmt::Arguments) {
    Stdout.write_fmt(args).unwrap();
}

/// Print! to the host console using the format string and arguments.
#[macro_export]
macro_rules! print {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::console::print(format_args!($fmt $(, $($arg)+)?))
    }
}

/// Println! to the host console using the format string and arguments.
#[macro_export]
macro_rules! println {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::console::print(format_args_nl!($fmt $(, $($arg)+)?))
    }
}
