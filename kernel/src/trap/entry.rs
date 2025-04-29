use super::*;
use core::arch::naked_asm;

macro_rules! load_or_save_all {
    ($op:literal) => { concat_instructions!(
        //    x0,  0*8(sp)   # zero, no need to save/load
        $op " x1,  1*8(sp)";
        //    x2,  2*8(sp) ; # %sp, load/save it later
        //    x3,  3*8(sp)   # %gp, not used
        //    x4,  4*8(sp)   # %tp, not used
        $op " x5,  5*8(sp)";
        $op " x6,  6*8(sp)";
        $op " x7,  7*8(sp)";
        $op " x8,  8*8(sp)";
        $op " x9,  9*8(sp)";
        $op "x10, 10*8(sp)";
        $op "x11, 11*8(sp)";
        $op "x12, 12*8(sp)";
        $op "x13, 13*8(sp)";
        $op "x14, 14*8(sp)";
        $op "x15, 15*8(sp)";
        $op "x16, 16*8(sp)";
        $op "x17, 17*8(sp)";
        $op "x18, 18*8(sp)";
        $op "x19, 19*8(sp)";
        $op "x20, 20*8(sp)";
        $op "x21, 21*8(sp)";
        $op "x22, 22*8(sp)";
        $op "x23, 23*8(sp)";
        $op "x24, 24*8(sp)";
        $op "x25, 25*8(sp)";
        $op "x26, 26*8(sp)";
        $op "x27, 27*8(sp)";
        $op "x28, 28*8(sp)";
        $op "x29, 29*8(sp)";
        $op "x30, 30*8(sp)";
        $op "x31, 31*8(sp)";
    ) };
}

macro_rules! load_or_save_callee {
    ($op:literal) => { concat_instructions!(
        $op " ra,  0*8(sp)";
        $op " s0,  1*8(sp)";
        $op " s1,  2*8(sp)";
        $op " s2,  3*8(sp)";
        $op " s3,  4*8(sp)";
        $op " s4,  5*8(sp)";
        $op " s5,  6*8(sp)";
        $op " s6,  7*8(sp)";
        $op " s7,  8*8(sp)";
        $op " s8,  9*8(sp)";
        $op " s9, 10*8(sp)";
        $op "s10, 11*8(sp)";
        $op "s11, 12*8(sp)";
    ) };
}

macro_rules! save_csr {
    () => {
        // Zero %sscratch so that we can identify traps from the kernel
        "
        csrrw t0, sscratch, zero
        csrr  t1, sstatus
        csrr  t2, sepc
        sd    t0,  2*8(sp)
        sd    t1, 32*8(sp)
        sd    t2, 33*8(sp)
        "
    };
}

macro_rules! load_csr {
    () => {
        "
        ld   t0, 32*8(sp)
        ld   t1, 33*8(sp)
        csrw sstatus, t0
        csrw sepc,    t1
        "
    };
}

#[repr(align(2))] // Required by RISC-V Specification
#[unsafe(naked)]
pub unsafe extern "C" fn trap_entry() {
    naked_asm!(
        // Save the trap context:
        // * For kernel traps, we push the context on the kernel stack.
        // * For user traps, the context is allocated on the kernel heap.
        "
        csrrw sp, sscratch, sp
        bnez  sp, {trap_from_user}
        csrr  sp, sscratch
        j     {trap_from_kernel}
        ",
        trap_from_kernel = sym trap_from_kernel,
        trap_from_user   = sym trap_from_user,
    );
}

#[unsafe(naked)]
unsafe extern "C" fn trap_from_kernel() {
    // Handle the kernel trap and return to where the exception occurred.
    naked_asm!(
        "
        addi sp, sp, -{context_size}
        ",
        load_or_save_all!("sd"),
        save_csr!(),
        "
        mv   a0, sp
        call {trap_handler}
        ",
        load_csr!(),
        load_or_save_all!("ld"),
        "
        addi sp, sp, {context_size}
        sret
        ",
        context_size = const core::mem::size_of::<TrapContext>(),
        trap_handler = sym   super::kernel_trap_handler,
    );
}

#[unsafe(naked)]
unsafe extern "C" fn trap_from_user() {
    naked_asm!(
        load_or_save_all!("sd"),
        save_csr!(),
        // Load kernel's stack
        "
        ld sp, (sp)
        ",
        // Load previously saved registers
        load_or_save_callee!("ld"),
        // Return to the caller of this task
        "
        addi sp, sp, 13*8
        ret
        ",
    );
}

#[unsafe(naked)]
pub unsafe extern "C" fn trap_return_to_user(cx: &mut TrapContext) {
    naked_asm!(
        // Save caller-saved registers
        "
        addi sp, sp, -13*8
        ",
        load_or_save_callee!("sd"),
        // Load user's execution context
        "
        csrw sscratch, a0
        sd   sp, (a0)  # Save kernel's stack
        mv   sp,  a0
        ",
        load_csr!(),
        load_or_save_all!("ld"),
        // Return to user mode
        "
        ld sp, 2*8(sp) # Load user's stack
        sret
        ",
    );
}
