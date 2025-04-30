#![no_std]
#![feature(linkage)]

#[macro_use]
pub mod console;
pub mod syscall;

pub use syscall::*;

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
    syscall::sys_exit(-1);
}

#[unsafe(link_section = ".text.entry")]
#[unsafe(no_mangle)]
unsafe extern "C" fn _start() -> ! {
    unsafe extern "C" {
        fn sbss();
        fn ebss();
    }
    // Clear the .bss segment
    unsafe {
        let sbss = sbss as usize;
        let ebss = ebss as usize;
        core::slice::from_raw_parts_mut::<u8>(sbss as *mut u8, ebss - sbss).fill(0);
    }
    syscall::sys_exit(main());
}

#[linkage = "weak"]
#[unsafe(no_mangle)]
fn main() -> i32 {
    panic!("main() is not implemented");
}
