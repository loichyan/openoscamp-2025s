mod entry;

use riscv::register::{scause, sstatus, stval};

#[derive(Debug)]
#[repr(C)]
pub struct TrapContext {
    /// `x[0]` is used to save kernel's `%sp`.
    x: [usize; 32],
    sstatus: usize,
    sepc: usize,
}

impl TrapContext {
    pub fn new_user(entrypoint: usize, stack_top: usize) -> Self {
        let mut regs = [0; 32];
        regs[2] = stack_top;

        let mut sstatus = sstatus::read();
        sstatus.set_spp(sstatus::SPP::Supervisor); // TODO: use user mode

        Self {
            x: regs,
            sstatus: sstatus.bits(),
            sepc: entrypoint,
        }
    }

    pub fn call(&mut self) -> scause::Scause {
        unsafe { entry::trap_return_to_user(self) };
        scause::read()
    }
}

#[allow(dead_code)]
#[rustfmt::skip]
impl TrapContext {
    pub const fn arg0(&self) -> usize { self.x[10] }
    pub const fn arg1(&self) -> usize { self.x[11] }
    pub const fn arg2(&self) -> usize { self.x[12] }
    pub const fn arg3(&self) -> usize { self.x[13] }
    pub const fn arg4(&self) -> usize { self.x[14] }
    pub const fn arg5(&self) -> usize { self.x[17] }

    pub const fn set_ret0(&mut self, val: usize) { self.x[10] = val; }
    pub const fn set_ret1(&mut self, val: usize) { self.x[11] = val; }
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
    panic!("trap from kernel:\n{cause:?} {stval:#x}\n{cx:#x?}");
}
