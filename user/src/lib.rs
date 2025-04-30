#![no_std]

pub mod syscall {
    use common::syscall::*;

    /// # Safety
    ///
    /// The caller must follow the specified syscall convention.
    pub unsafe fn syscall(id: usize, args: [usize; 3]) -> isize {
        let ret: isize;
        unsafe {
            core::arch::asm!(
                "ecall",
                inlateout("a0") args[0] => ret,
                in("a1")        args[1],
                in("a2")        args[2],
                in("a7")        id,
            );
        }
        ret
    }

    pub fn sys_yield() -> isize {
        unsafe { syscall(SYS_YIELD, [0; 3]) }
    }

    pub fn sys_exit(code: i32) -> isize {
        unsafe { syscall(SYS_EXIT, [code as usize, 0, 0]) }
    }

    pub fn sys_write(fd: usize, bytes: &[u8]) -> isize {
        unsafe { syscall(SYS_EXIT, [fd, bytes.as_ptr() as usize, bytes.len()]) }
    }
}
pub use syscall::*;
