use crate::task::{Task, TaskState};
use common::syscall::*;

pub fn handle(task: &mut Task) {
    let cx = &task.cx;
    let ret = match cx.arg5() {
        SYS_WRITE => sys_write(cx.arg0(), cx.arg1() as *const u8, cx.arg2()),
        SYS_EXIT => {
            task.state = TaskState::Exited;
            log::info!("user exited with code: {}", cx.arg0() as i32);
            return;
        },
        SYS_YIELD => 0,
        id => panic!("unsupported syscall: {id}"),
    };
    task.cx.set_ret1(ret as usize);
}

fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    match fd {
        FD_STDOUT => {
            let bytes = unsafe { core::slice::from_raw_parts(buf, len) };
            if crate::console::write_bytes(bytes).is_err() {
                -1
            } else {
                len as isize
            }
        },
        _ => panic!("unsupported fd: {fd}"),
    }
}
