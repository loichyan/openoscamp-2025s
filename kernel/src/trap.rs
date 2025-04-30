mod entry;

use riscv::register::{scause, stval};

#[derive(Debug, Default)]
#[repr(C)]
pub struct TrapContext {
    /// `x[0]` is used to save kernel's `%sp`.
    pub x: [usize; 32],
    pub sstatus: usize,
    pub sepc: usize,
}

impl TrapContext {
    pub fn set_sp(&mut self, stack_top: usize) {
        self.x[2] = stack_top;
    }

    pub fn call(&mut self) {
        unsafe { entry::trap_return_to_user(self) };
        user_trap_handler(self);
    }
}

pub unsafe fn init() {
    unsafe {
        riscv::register::stvec::write(
            entry::trap_entry as usize,
            riscv::register::stvec::TrapMode::Direct,
        )
    }
}

extern "C" fn kernel_trap_handler(cx: &mut TrapContext) {
    let cause = scause::read().cause();
    let stval = stval::read();
    // TODO: handle kernel traps
    panic!("trap from kernel:\n{cause:?} {stval:#x}\n{cx:#?}");
}

extern "C" fn user_trap_handler(cx: &mut TrapContext) {
    let cause = scause::read().cause();
    let stval = stval::read();
    // TODO: handle user traps
    log::debug!("trap from user:\n{cause:?} {stval:#x}\n{cx:#?}");
}
