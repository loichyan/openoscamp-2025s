#![no_std]
#![no_main]
#![feature(sync_unsafe_cell)]

use self::sbi::shutdown;

#[macro_use]
mod console;
mod boot;
mod config;
mod mm;
mod sbi;

#[panic_handler]
fn panic_handler(info: &core::panic::PanicInfo) -> ! {
    if let Some(loc) = info.location() {
        console::print_silent(format_args!(
            "[kernel] panicked at {}:{} {}",
            loc.file(),
            loc.line(),
            info.message()
        ));
    } else {
        console::print_silent(format_args!("[kernel] panicked: {}", info.message()));
    }
    sbi::shutdown(1);
}

fn main() -> ! {
    unsafe extern "C" {
        fn stext();
        fn etext();
        fn srodata();
        fn erodata();
        fn sdata();
        fn edata();
        fn sbss();
        fn ebss();
    }
    println!(
        "[kernel] .text   [{:#x}, {:#x})",
        stext as usize, etext as usize
    );
    println!(
        "[kernel] .rodata [{:#x}, {:#x})",
        srodata as usize, erodata as usize
    );
    println!(
        "[kernel] .data   [{:#x}, {:#x})",
        sdata as usize, edata as usize
    );
    println!(
        "[kernel] .bss    [{:#x}, {:#x})",
        sbss as usize, ebss as usize
    );

    println!("Hello, world!");

    shutdown(0);
}
