mod entry;

use riscv::register::{scause, sstatus, stval};

#[derive(Debug)]
#[repr(C)]
pub struct TrapContext {
    caller: CallerRegs,
    callee: CalleeRegs,
    csr: CsrRegs,
    /// User's stack pointer
    sp: usize,
    /// Kernel's stack pointer
    ksp: usize,
}

#[derive(Debug, Default)]
#[repr(C)]
struct CallerRegs {
    sp: usize,
    ra: usize,

    a0: usize,
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,

    t0: usize,
    t1: usize,
    t2: usize,
    t3: usize,
    t4: usize,
    t5: usize,
    t6: usize,
}

#[derive(Debug, Default)]
#[repr(C)]
struct CalleeRegs {
    s0: usize,
    s1: usize,
    s2: usize,
    s3: usize,
    s4: usize,
    s5: usize,
    s6: usize,
    s7: usize,
    s8: usize,
    s9: usize,
    s10: usize,
    s11: usize,
}

#[derive(Debug, Default)]
#[repr(C)]
struct CsrRegs {
    sstatus: usize,
    sepc: usize,
}

impl TrapContext {
    pub fn new_user(entrypoint: usize, stack_top: usize) -> Self {
        Self {
            caller: CallerRegs::default(),
            callee: CalleeRegs::default(),
            csr: CsrRegs {
                sstatus: {
                    let mut sstatus = sstatus::read();
                    sstatus.set_spp(sstatus::SPP::Supervisor); // TODO: use user mode
                    sstatus.bits()
                },
                sepc: entrypoint,
            },
            sp: stack_top,
            ksp: 0,
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
    pub const fn arg0(&self) -> usize { self.caller.a0 }
    pub const fn arg1(&self) -> usize { self.caller.a1 }
    pub const fn arg2(&self) -> usize { self.caller.a2 }
    pub const fn arg3(&self) -> usize { self.caller.a3 }
    pub const fn arg4(&self) -> usize { self.caller.a4 }
    pub const fn arg5(&self) -> usize { self.caller.a7 }

    pub const fn set_ret0(&mut self, val: usize) { self.caller.a0 = val; }
    pub const fn set_ret1(&mut self, val: usize) { self.caller.a1 = val; }
}

pub unsafe fn init() {
    unsafe {
        riscv::register::stvec::write(
            entry::trap_entry as usize,
            riscv::register::stvec::TrapMode::Direct,
        )
    }
}

extern "C" fn kernel_trap_handler() {
    let cause = scause::read().cause();
    let stval = stval::read();
    // TODO: handle kernel traps
    panic!("trap from kernel: {cause:x?} {stval:#x}");
}
