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

    test_user_trap();

    shutdown(0);
}

fn test_user_trap() {
    static mut USER_STACK: [u8; 4096] = [0; 4096];

    let mut sstatus = riscv::register::sstatus::read();
    sstatus.set_spp(riscv::register::sstatus::SPP::Supervisor);

    let mut cx = trap::TrapContext {
        sstatus: sstatus.bits(),
        sepc: user_app as usize,
        ..Default::default()
    };
    cx.set_sp(&raw mut USER_STACK as usize + 4096);

    cx.call();
}

extern "C" fn user_app() {
    println!("[user] Hello, kernel!");
    unsafe {
        core::ptr::read_volatile(0x1000 as *const usize);
    }
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
