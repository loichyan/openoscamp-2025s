use crate::task::*;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::{Rc, Weak};
use std::task::{ContextBuilder, LocalWaker};

type RunQueue = RefCell<VecDeque<TaskRef>>;

pub struct Executor {
    queue: Rc<RunQueue>,
}

impl Executor {
    pub fn new() -> Self {
        Self {
            queue: Rc::new(RefCell::new(VecDeque::new())),
        }
    }

    pub fn spawn<T, F>(&self, fut: F) -> Task<T>
    where
        T: 'static,
        F: 'static + Future<Output = T>,
    {
        let waker = TaskWaker(Rc::downgrade(&self.queue));
        let task = Task::new(waker, fut);
        self.queue.borrow_mut().push_back(task.get_ref());
        task
    }

    pub fn run(&self) {
        while let Some(task) = { self.queue.borrow_mut().pop_front() } {
            let waker = LocalWaker::from(task.clone());
            let mut cx = ContextBuilder::from_waker(core::task::Waker::noop())
                .local_waker(&waker)
                .build();
            if task.poll(&mut cx).is_pending() {
                self.queue.borrow_mut().push_back(task);
            }
        }
    }
}

impl Default for Executor {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) struct TaskWaker(Weak<RunQueue>);

impl TaskWaker {
    pub fn wake(&self, task: TaskRef) {
        self.0
            .upgrade()
            .expect("Executor has been disposed")
            .borrow_mut()
            .push_back(task);
    }
}
