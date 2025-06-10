use crate::task::*;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::pin::pin;
use std::rc::{Rc, Weak};
use std::task::{Context, ContextBuilder, LocalWaker, Poll, Waker};

pub fn spawn<T, F>(fut: F) -> Task<T>
where
    T: 'static,
    F: 'static + Future<Output = T>,
{
    let task = Task::new(fut);
    ExecutorHandle::wake(task.inner());
    task
}

pub async fn yield_now() {
    let mut polled = false;
    std::future::poll_fn(|cx| {
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

pub struct Executor(Rc<ExecutorInner>);

struct ExecutorInner {
    queue: RefCell<VecDeque<TaskRef>>,
}

impl Executor {
    pub fn new() -> Self {
        let inner = ExecutorInner {
            queue: RefCell::new(VecDeque::new()),
        };
        Self(Rc::new(inner))
    }

    pub fn block_on<T>(&self, fut: impl Future<Output = T>) -> T {
        let _guard = ExecutorHandle::enter(&self.0);
        let ExecutorInner { queue } = &*self.0;
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
                let waker = LocalWaker::from(task.clone());
                let mut cx = ContextBuilder::from_waker(Waker::noop())
                    .local_waker(&waker)
                    .build();
                _ = task.poll(&mut cx);
            }
        }
    }
}

impl Default for Executor {
    fn default() -> Self {
        Self::new()
    }
}

thread_local! {
    static CX: RefCell<Weak<ExecutorInner>> = const { RefCell::new(Weak::new()) };
}

pub(crate) struct ExecutorHandle;

impl ExecutorHandle {
    fn get() -> Rc<ExecutorInner> {
        CX.with_borrow(Weak::upgrade)
            .expect("not inside a valid executor")
    }

    fn enter(cx: &Rc<ExecutorInner>) -> impl Drop {
        struct Revert;
        impl Drop for Revert {
            fn drop(&mut self) {
                CX.with_borrow_mut(|d| *d = Weak::new())
            }
        }
        CX.with_borrow_mut(|d| {
            if d.strong_count() != 0 {
                panic!("cannot run within a nested executor")
            }
            *d = Rc::downgrade(cx)
        });
        Revert
    }

    pub(crate) fn wake(task: TaskRef) {
        Self::get().queue.borrow_mut().push_back(task);
    }
}
