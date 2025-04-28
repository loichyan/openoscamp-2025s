mod entry;

use crate::config::*;

#[repr(transparent)]
struct KernelStack {
    data: [u8; KERNEL_STACK_SIZE],
}

#[repr(transparent)]
struct UserStack {
    data: [u8; USER_STACK_SIZE],
}

#[derive(Debug)]
#[repr(C)]
struct TrapFrame {
    /// `x[0]` ise used to save the kernel program counter.
    pub x: [usize; 32],
    pub sstatus: usize,
    pub sepc: usize,
}

pub unsafe fn init() {
    unsafe {
        riscv::register::stvec::write(
            entry::trap_entry as usize,
            riscv::register::utvec::TrapMode::Direct,
        )
    }
}

extern "C" fn kernel_trap_handler(cx: &mut TrapFrame) {
    let cause = riscv::register::scause::read().cause();
    let stval = riscv::register::stval::read();
    // TODO: handle kernel traps
    panic!("trap from kernel:\n{cause:?} {stval:#x}\n{cx:#?}");
}

extern "C" fn trap_handler(_cx: &mut TrapFrame) {}
