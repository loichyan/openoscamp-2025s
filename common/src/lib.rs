#![no_std]

#[rustfmt::skip]
pub mod syscall {
    pub const FD_STDOUT: usize = 1;

    pub const SYS_WRITE: usize = 64;
    pub const SYS_EXIT:  usize = 93;
    pub const SYS_YIELD: usize = 124;
}
