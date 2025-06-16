use alloc::collections::VecDeque;
use core::cell::RefCell;
use core::pin::pin;
use core::task::{Context, Poll, Waker};

use crate::task::*;

pub fn spawn<Ex, T, F>(handle: Ex, fut: F) -> Task<T>
where
    T: 'static,
    F: 'static + Future<Output = T>,
    Ex: ExecutorHandle,
{
    let ex = handle.get();
    let task = Task::new(handle, fut);
    ex.wake(task.inner());
    task
}

pub async fn yield_now() {
    let mut polled = false;
    core::future::poll_fn(|cx| {
        if polled {
            Poll::Ready(())
        } else {
            polled = true;
            cx.local_waker().wake_by_ref();
            Poll::Pending
        }
    })
    .await
}

pub struct Executor {
    queue: RefCell<VecDeque<TaskRef>>,
}

impl Executor {
    pub fn new() -> Self {
        Self {
            queue: RefCell::new(VecDeque::new()),
        }
    }

    pub(crate) fn wake(&self, task: TaskRef) {
        self.queue.borrow_mut().push_back(task);
    }

    pub fn block_on<T>(&self, fut: impl Future<Output = T>) -> T {
        // let _guard = ExecutorHandle::enter(&self.0);
        let Self { queue } = self;
        let mut cx = Context::from_waker(Waker::noop());
        let mut fut = pin!(fut);
        loop {
            if let Poll::Ready(output) = fut.as_mut().poll(&mut cx) {
                // Remove all spawned tasks.
                queue.borrow_mut().clear();
                return output;
            }

            // Newly waked tasks are deferred to the next loop.
            let count = queue.borrow().len();
            for _ in 0..count {
                let task = queue.borrow_mut().pop_front().unwrap();
                _ = task.poll_wakeable();
            }
        }
    }
}

impl Default for Executor {
    fn default() -> Self {
        Self::new()
    }
}

pub trait ExecutorHandle: 'static + Unpin {
    type Ref: core::ops::Deref<Target = Executor>;

    fn get(&self) -> Self::Ref;
}
impl ExecutorHandle for alloc::rc::Weak<Executor> {
    type Ref = alloc::rc::Rc<Executor>;
    fn get(&self) -> Self::Ref {
        self.upgrade().expect("not inside a valid executor")
    }
}
