use crate::trap::TrapContext;

#[derive(Debug)]
pub struct Task {
    pub cx: TrapContext,
    pub state: TaskState,
}

impl Task {
    pub fn new(entrypoint: usize, stack_top: usize) -> Self {
        Self {
            cx: crate::trap::TrapContext::new_user(entrypoint, stack_top),
            state: TaskState::Ready,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskState {
    Ready,
    Exited,
}
