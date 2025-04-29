#![no_std]
#![no_main]
#![feature(debug_closure_helpers)]
#![feature(fn_align)]
#![feature(format_args_nl)]
#![feature(sync_unsafe_cell)]

use self::sbi::shutdown;
use log::info;

#[macro_use]
mod utils;
#[macro_use]
mod console;
mod boot;
mod config;
mod logging;
mod mm;
mod sbi;
mod trap;

#[panic_handler]
fn panic_handler(info: &core::panic::PanicInfo) -> ! {
    if let Some(loc) = info.location() {
        console::print_silent(format_args!(
            "[kernel] panicked at {}:{}: {}\n",
            loc.file(),
            loc.line(),
            info.message()
        ));
    } else {
        console::print_silent(format_args!("[kernel] panicked: {}\n", info.message()));
    }
    sbi::shutdown(1);
}

fn main() -> ! {
    unsafe {
        logging::init();
        trap::init();
    }
    show_segments();
    println!("Hello, world!");

    shutdown(0);
}

// TODO: create a more safe kernel page table
fn show_segments() {
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
    info!(".text   [{:#x}, {:#x})", stext as usize, etext as usize);
    info!(".rodata [{:#x}, {:#x})", srodata as usize, erodata as usize);
    info!(".data   [{:#x}, {:#x})", sdata as usize, edata as usize);
    info!(".bss    [{:#x}, {:#x})", sbss as usize, ebss as usize);
}
