use super::*;
use crate::asm::*;
use core::arch::naked_asm;
use core::mem::{offset_of, size_of};

macro_rules! offset_of_context {
    ($($f:tt)*) => { (offset_of!(TrapContext, $($f)*) / size_of::<usize>()) };
}

#[rustfmt::skip]
macro_rules! caller_regs {
    ($op:ident, $r:ident[$i:tt]) => {
        concat_asm!(
            $op!(ra, $r[$i+0]),

            $op!(a0, $r[$i+1]),
            $op!(a1, $r[$i+2]),
            $op!(a2, $r[$i+3]),
            $op!(a3, $r[$i+4]),
            $op!(a4, $r[$i+5]),
            $op!(a5, $r[$i+6]),
            $op!(a6, $r[$i+7]),
            $op!(a7, $r[$i+8]),

            $op!(t0, $r[$i+9]),
            $op!(t1, $r[$i+10]),
            $op!(t2, $r[$i+11]),
            $op!(t3, $r[$i+12]),
            $op!(t4, $r[$i+13]),
            $op!(t5, $r[$i+14]),
            $op!(t6, $r[$i+15]),
        )
    };
}

#[rustfmt::skip]
macro_rules! callee_regs {
    ($op:ident, $r:ident[$i:tt]) => {
        concat_asm!(
            $op!(s0,  $r[$i+0]),
            $op!(s1,  $r[$i+1]),
            $op!(s2,  $r[$i+2]),
            $op!(s3,  $r[$i+3]),
            $op!(s4,  $r[$i+4]),
            $op!(s5,  $r[$i+5]),
            $op!(s6,  $r[$i+6]),
            $op!(s7,  $r[$i+7]),
            $op!(s8,  $r[$i+8]),
            $op!(s9,  $r[$i+9]),
            $op!(s10, $r[$i+10]),
            $op!(s11, $r[$i+11]),
        )
    };
}

#[rustfmt::skip]
macro_rules! csr_regs {
    (save, $r:ident[$i:tt]) => {
        concat_asm!(
            "csrr t0, sstatus",
            "csrr t1, sepc",
            save!(t0, $r[$i+0]),
            save!(t1, $r[$i+1]),
        )
    };
    (load, $r:ident[$i:tt]) => {
        concat_asm!(
            load!(t0, $r[$i+0]),
            load!(t1, $r[$i+1]),
            "csrw sstatus, t0",
            "csrw sepc,    t1",
        )
    };
}

macro_rules! trap_context {
    (save, $r:ident) => {
        concat_asm!(
            caller_regs!(save, $r[offset_caller]),
            callee_regs!(save, $r[offset_callee]),
            csr_regs!(save, $r[offset_csr]),
            save!(sp, $r[offset_sp]),
        )
    };
    (load, $r:ident) => {
        concat_asm!(
            csr_regs!(load, $r[offset_csr]),
            caller_regs!(load, $r[offset_caller]),
            callee_regs!(load, $r[offset_callee]),
            load!(sp, $r[offset_sp]),
        )
    };
}

macro_rules! trap_asm {
    ($($args:tt)*) => {
        naked_asm!(
            // Pretend all named arguments are used
            "
            // {size_context}
            // {size_caller}
            // {size_callee_1}
            // {offset_sp}
            // {offset_ksp}
            // {offset_csr}
            // {offset_caller}
            // {offset_callee}
            ",

            $($args)*

            size_context  = const size_of::<TrapContext>(),
            size_caller   = const size_of::<CallerRegs>(),
            size_callee_1 = const size_of::<CalleeRegs>() + size_of::<usize>(),
            offset_caller = const offset_of_context!(caller),
            offset_callee = const offset_of_context!(callee),
            offset_csr    = const offset_of_context!(csr),
            offset_sp     = const offset_of_context!(sp),
            offset_ksp    = const offset_of_context!(ksp),

        )
    }
}

#[repr(align(2))] // Required by RISC-V Specification
#[unsafe(naked)]
pub unsafe extern "C" fn trap_entry() {
    naked_asm!(
        // Save the trap context:
        // * For kernel traps, we push the context on the kernel stack.
        // * For user traps, the context is allocated on the kernel heap.
        "csrrw sp, sscratch, sp",
        "bnez sp, {trap_from_user}",
        "csrr sp, sscratch",
        "j {trap_from_kernel}",

        trap_from_kernel = sym trap_from_kernel,
        trap_from_user   = sym trap_from_user,
    );
}

#[unsafe(naked)]
unsafe extern "C" fn trap_from_kernel() {
    trap_asm!(
        // 1) Save kernel's execution context
        "addi sp, sp, -{size_context}",
        trap_context!(save, sp),
        // 2) Call the trap handler
        "call {trap_handler}",
        // 3) Load kernel's execution context
        trap_context!(load, sp),
        "addi sp, sp, {size_context}",
        // 4) Return to where the exception occurred
        "sret",

        trap_handler = sym super::kernel_trap_handler,
    );
}

#[unsafe(naked)]
unsafe extern "C" fn trap_from_user() {
    trap_asm!(
        // 1) Save user's execution context
        trap_context!(save, sp),
        // 2) Load kernel's stack
        load!(sp, sp[offset_ksp]),
        // 3) Load previously saved registers
        load!(ra, sp[0]),          // %sp[0]    : %ra
        callee_regs!(load, sp[1]), // %sp[1..12]: %s0..%s11
        "addi sp, sp, {size_callee_1}",
        // Return to the caller of this task
        "ret",
    );
}

#[unsafe(naked)]
pub unsafe extern "C" fn trap_return_to_user(cx: &mut TrapContext) {
    trap_asm!(
        // 1) Save caller-saved registers
        "addi sp, sp, -{size_callee_1}",
        save!(ra, sp[0]),          // %sp[0]    : %ra
        callee_regs!(save, sp[1]), // %sp[1..12]: %s0..%s11
        // 2) Save kernel's stack and context pointer
        "csrw sscratch, a0",
        save!(sp, a0[offset_ksp]),
        "mv sp, a0",
        // 3) Load user's execution context
        trap_context!(load, sp),
        // Return to user mode
        "sret",
    );
}
