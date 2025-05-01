use super::*;
use crate::asm::*;
use core::arch::naked_asm;

#[rustfmt::skip]
macro_rules! __load_or_save_context {
    ($op:ident) => {
        concat_asm!(
            // %x0, no need to save/load
            $op!(x1,  sp[1]),
            // %sp, load/save it later
            // %gp, not used
            // %tp, not used
            $op!(x5,  sp[5]),
            $op!(x6,  sp[6]),
            $op!(x7,  sp[7]),
            $op!(x8,  sp[8]),
            $op!(x9,  sp[9]),
            $op!(x10, sp[10]),
            $op!(x11, sp[11]),
            $op!(x12, sp[12]),
            $op!(x13, sp[13]),
            $op!(x14, sp[14]),
            $op!(x15, sp[15]),
            $op!(x16, sp[16]),
            $op!(x17, sp[17]),
            $op!(x18, sp[18]),
            $op!(x19, sp[19]),
            $op!(x20, sp[20]),
            $op!(x21, sp[21]),
            $op!(x22, sp[22]),
            $op!(x23, sp[23]),
            $op!(x24, sp[24]),
            $op!(x25, sp[25]),
            $op!(x26, sp[26]),
            $op!(x27, sp[27]),
            $op!(x28, sp[28]),
            $op!(x29, sp[29]),
            $op!(x30, sp[30]),
            $op!(x31, sp[31]),
        )
    };
}

#[rustfmt::skip]
macro_rules! __load_or_save_callee {
    ($op:ident) => {
        concat_asm!(
            $op!(ra,  sp[0]),
            $op!(s0,  sp[1]),
            $op!(s1,  sp[2]),
            $op!(s2,  sp[3]),
            $op!(s3,  sp[4]),
            $op!(s4,  sp[5]),
            $op!(s5,  sp[6]),
            $op!(s6,  sp[7]),
            $op!(s7,  sp[8]),
            $op!(s8,  sp[9]),
            $op!(s9,  sp[10]),
            $op!(s10, sp[11]),
            $op!(s11, sp[12]),
        )
    };
}

macro_rules! save_context {
    () => {
        concat_asm!(
            __load_or_save_context!(save),
            "csrrw t0, sscratch, zero",
            "csrr t1, sstatus",
            "csrr t2, sepc",
            save!(t0, sp[2]),
            save!(t1, sp[32]),
            save!(t2, sp[33]),
        )
    };
}
macro_rules! load_context {
    () => {
        concat_asm!(
            load!(t0, sp[32]),
            load!(t1, sp[33]),
            "csrw sstatus, t0",
            "csrw sepc,    t1",
            __load_or_save_context!(load),
        )
    };
}

macro_rules! load_callee {
    () => {
        __load_or_save_callee!(load)
    };
}
macro_rules! save_callee {
    () => {
        __load_or_save_callee!(save)
    };
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
    // Handle the kernel trap and return to where the exception occurred.
    naked_asm!(
        "addi sp, sp, -{context_size}",
        save_context!(),
        "mv a0, sp",
        "call {trap_handler}",
        load_context!(),
        "addi sp, sp, {context_size}",
        "sret",
        context_size = const size_of::<TrapContext>(),
        trap_handler = sym   super::kernel_trap_handler,
    );
}

#[unsafe(naked)]
unsafe extern "C" fn trap_from_user() {
    naked_asm!(
        // Save user's execution context
        save_context!(),
        // Load kernel's stack
        load!(sp, sp[0]),
        // Load previously saved registers
        load_callee!(),
        // Return to the caller of this task
        "addi sp, sp, {callee_context_size}",
        "ret",
        callee_context_size = const size_of::<[usize; 13]>(),
    );
}

#[unsafe(naked)]
pub unsafe extern "C" fn trap_return_to_user(cx: &mut TrapContext) {
    naked_asm!(
        // Save caller-saved registers
        "addi sp, sp, -{callee_context_size}",
        save_callee!(),
        // Load user's execution context
        "csrw sscratch, a0",
        save!(sp, a0[0]), // Save kernel's stack
        "mv sp, a0",
        load_context!(),
        // Return to user mode
        load!(sp, sp[2]), // Load user's stack
        "sret",
        callee_context_size = const size_of::<[usize; 13]>(),
    );
}
