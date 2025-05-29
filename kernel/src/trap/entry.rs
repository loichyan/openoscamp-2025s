use super::*;
use crate::asm::*;
use core::arch::naked_asm;
use core::mem::{offset_of, size_of};

macro_rules! offset_in_usize {
    ($($args:tt)*) => { (offset_of!($($args)*) / size_of::<usize>()) };
}

#[rustfmt::skip]
macro_rules! caller_regs_except_a0 {
    ($op:ident, $r:ident[$i:tt]) => {
        concat_asm!(
            // !(a0, $r[$i+0]),
            $op!(a1, $r[$i+1]),
            $op!(a2, $r[$i+2]),
            $op!(a3, $r[$i+3]),
            $op!(a4, $r[$i+4]),
            $op!(a5, $r[$i+5]),
            $op!(a6, $r[$i+6]),
            $op!(a7, $r[$i+7]),

            $op!(t0, $r[$i+8]),
            $op!(t1, $r[$i+9]),
            $op!(t2, $r[$i+10]),
            $op!(t3, $r[$i+11]),
            $op!(t4, $r[$i+12]),
            $op!(t5, $r[$i+13]),
            $op!(t6, $r[$i+14]),

            $op!(ra, $r[$i+15]),
        )
    };
}

#[rustfmt::skip]
macro_rules! caller_regs {
    ($op:ident, $r:ident[$i:tt]) => {
        concat_asm!(
            $op!(a0, $r[$i+0]),
            caller_regs_except_a0!($op, $r[$i]),
        )
    };
}

#[rustfmt::skip]
macro_rules! callee_regs_except_s0 {
    ($op:ident, $r:ident[$i:tt]) => {
        concat_asm!(
            // !(s0,  $r[$i+0]),
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
macro_rules! callee_regs {
    ($op:ident, $r:ident[$i:tt]) => {
        concat_asm!(
            $op!(s0, $r[$i+0]),
            callee_regs_except_s0!($op, $r[$i]),
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

macro_rules! trap_asm {
    ($($args:tt)*) => {
        naked_asm!(
            // Pretend all named arguments are used
            "
            // {TrapContext_size}
            // {CallerRegs_size}
            // {CalleeRegs_size}
            // {TempContext_size}
            //
            // {TrapContext_caller}
            // {TrapContext_callee}
            // {TrapContext_csr}
            // {TrapContext_sp}
            // {TrapContext_ksp}
            //
            // {TempContext_ra}
            // {TempContext_callee}
            ",

            $($args)*

            TrapContext_size   = const size_of::<TrapContext>(),
            TempContext_size   = const size_of::<TempContext>(),
            CallerRegs_size    = const size_of::<CallerRegs>(),
            CalleeRegs_size    = const size_of::<CalleeRegs>(),

            TrapContext_caller = const offset_in_usize!(TrapContext, caller),
            TrapContext_callee = const offset_in_usize!(TrapContext, callee),
            TrapContext_csr    = const offset_in_usize!(TrapContext, csr),
            TrapContext_sp     = const offset_in_usize!(TrapContext, sp),
            TrapContext_ksp    = const offset_in_usize!(TrapContext, ksp),

            TempContext_ra     = const offset_in_usize!(TempContext, ra),
            TempContext_callee = const offset_in_usize!(TempContext, callee),
        )
    }
}

/// A two-stage trap handler inspired by [`fast-trap`](https://github.com/rustsbi/fast-trap).
#[repr(align(2))] // Required by RISC-V Specification,
#[unsafe(naked)]
pub unsafe extern "C" fn trap_entry() {
    trap_asm!(
        // * For kernel traps, we push the context on the kernel stack.
        // * For user traps, the context is allocated on the kernel heap.
        "csrrw s0, sscratch, s0",
        "bnez s0, {user_entry}",
        // Handle kernel traps. When an exception occurs, the trap handling flow
        // is equivalent to inserting a function call to where the exception
        // occurs, so we only need to save caller-saved registers.
        //
        // Save kernel's execution context
        "csrrw s0, sscratch, zero", // Restore %s0
        "addi sp, sp, -{CallerRegs_size}",
        caller_regs!(save, sp[0]),
        // Call the trap handler
        "call {kernel_handler}",
        // Load kernel's execution context
        caller_regs!(load, sp[0]),
        "addi sp, sp, {CallerRegs_size}",
        // Return to where the exception occurred
        "sret",

        kernel_handler = sym super::kernel_handler,
        user_entry     = sym user_trap_entry,
    );
}

#[unsafe(naked)]
unsafe extern "C" fn user_trap_entry() {
    trap_asm!(
        // Handle user traps. We first try fast handler, which is is essentially
        // inserting a function call to where the trap occurs and the compiler
        // will help us persist the callee-saved registers.
        // If the fast handler fails, we delegate the handling to the scheduler,
        // which requires relatively more load/store operations to restore the
        // execution context.
        //
        // Save user's caller registers and stack pointer
        caller_regs!(save, s0[TrapContext_caller]), // %s0 : *mut TrapContext
        csr_regs!(save, s0[TrapContext_csr]),
        "csrrw t0, sscratch, zero",        // Zero sscratch to identify kernel exceptions
        save!(t0, s0[TrapContext_callee]), // Save %s0
        save!(sp, s0[TrapContext_sp]),     // Save user's stack pointer
        // Prepare  context for kernel calls
        load!(sp, s0[TrapContext_ksp]),    // Load kernel's stack pointer
        "mv a0, s0",                       // %a0: *mut TrapContext
        "call {user_fast_handler}",
        "bnez a0, {return_to_scheduler}",  // Fast handler failed
        // Fast handler succeeded.
        // Set %sscratch to identify the subsequent user trap.
        "csrw sscratch, s0", // %sscratch : *mut TrapContext
        csr_regs!(load, s0[TrapContext_csr]),
        caller_regs!(load, s0[TrapContext_caller]),
        load!(sp, s0[TrapContext_sp]),     // Load user's stack pointer
        load!(s0, s0[TrapContext_callee]), // Load %s0
        // Return to user mode
        "sret",

        user_fast_handler   = sym super::user_fast_handler,
        return_to_scheduler = sym return_to_scheduler,
    );
}

#[unsafe(naked)]
unsafe extern "C" fn return_to_scheduler(cx: &mut TrapContext) {
    trap_asm!(
        // Save all other execution context
        callee_regs_except_s0!(save, s0[TrapContext_callee]),
        // Load previously saved scheduler context
        load!(ra, sp[TempContext_ra]), // %sp is still kernel's; %ra : scheduler
        callee_regs!(load, sp[TempContext_callee]),
        "addi sp, sp, {TempContext_size}",
        // Return to the scheduler of this task
        "ret",
    );
}

#[unsafe(naked)]
pub unsafe extern "C" fn return_to_user(cx: &mut TrapContext) {
    trap_asm!(
        // Save scheduler context
        "addi sp, sp, -{TempContext_size}",
        save!(ra, sp[TempContext_ra]),
        callee_regs!(save, sp[TempContext_callee]),
        // Set %sscratch to identify the subsequent user trap
        "csrw sscratch, a0",            // %sscratch : *mut TrapContext
        save!(sp, a0[TrapContext_ksp]), // Save kernel's statck pointer
        // Load user's execution context
        csr_regs!(load, a0[TrapContext_csr]),
        caller_regs_except_a0!(load, a0[TrapContext_caller]),
        callee_regs!(load, a0[TrapContext_callee]),
        load!(sp, a0[TrapContext_sp]),     // Load user's stack pointer
        load!(a0, a0[TrapContext_caller]), // Load %a0
        // Return to user mode
        "sret",
    );
}

/// Temporary context of a scheduler call.
#[repr(C)]
struct TempContext {
    ra: usize,
    callee: CalleeRegs,
}
