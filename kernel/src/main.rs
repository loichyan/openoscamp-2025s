#![no_std]
#![no_main]
#![feature(debug_closure_helpers)]
#![feature(fn_align)]
#![feature(format_args_nl)]
#![feature(sync_unsafe_cell)]

use self::sbi::shutdown;
use log::info;
use riscv::register::stval;

#[macro_use]
mod console;
mod asm;
mod boot;
mod config;
mod logging;
mod mm;
mod sbi;
mod syscall;
mod task;
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
    use riscv::register::scause::{Exception, Trap};

    static mut USER_STACK: [u8; 4096] = [0; 4096];

    let mut task = task::Task::new(user_app as usize, &raw mut USER_STACK as usize + 4096);
    loop {
        match task.cx.call().cause() {
            Trap::Exception(Exception::UserEnvCall) => syscall::handle(&mut task),
            other => panic!(
                "unsupported exception: {other:x?} {:#x}\n{:#x?}",
                stval::read(),
                task.cx,
            ),
        }
        if task.state == task::TaskState::Exited {
            info!("user exited");
            break;
        }
    }
}

/// This is not an actual userland app. Instead, it is used to test user traps.
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
