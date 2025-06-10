use crate::executor::ExecutorHandle;
use pin_project_lite::pin_project;
use std::any::Any;
use std::cell::RefCell;
use std::marker::PhantomData;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, LocalWake, Poll, ready};

pub struct Task<T> {
    inner: TaskRef,
    marker: PhantomData<T>,
}

impl<T> Task<T> {
    pub(crate) fn new<F>(fut: F) -> Self
    where
        T: 'static,
        F: 'static + Future<Output = T>,
    {
        let task = WakeableTask {
            task: RefCell::new(Box::pin(TaskImpl::Pending { fut })),
        };
        Self {
            inner: Rc::new(task),
            marker: PhantomData,
        }
    }

    pub(crate) fn inner(&self) -> TaskRef {
        self.inner.clone()
    }

    pub fn abort(self) {
        self.inner.task.borrow_mut().as_mut().abort();
    }
}

impl<T: 'static> Future for Task<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        self.inner
            .task
            .borrow_mut()
            .as_mut()
            .poll(cx)
            .map(|o| o.downcast_mut::<Option<T>>().unwrap().take().unwrap())
    }
}

pub(crate) type TaskRef = Rc<WakeableTask>;

pub(crate) struct WakeableTask {
    // TODO: Use handcrafted vtable to eliminate memory indirections
    task: RefCell<Pin<Box<dyn AnyTask>>>,
}

impl WakeableTask {
    pub fn poll(&self, cx: &mut Context) -> Poll<()> {
        self.task.borrow_mut().as_mut().poll(cx).map(|_| ())
    }
}

impl LocalWake for WakeableTask {
    fn wake(self: Rc<Self>) {
        ExecutorHandle::wake(self);
    }
}

trait AnyTask {
    fn abort(self: Pin<&mut Self>);
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<&mut dyn Any>;
}

pin_project! {
    #[project = TaskPoll]
    enum TaskImpl<T, F> {
        Ready { output: Option<T> },
        Pending { #[pin] fut: F },
    }
}

impl<T, F> AnyTask for TaskImpl<T, F>
where
    T: 'static,
    F: Future<Output = T>,
{
    fn abort(mut self: Pin<&mut Self>) {
        self.set(TaskImpl::Ready { output: None });
    }

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<&mut dyn Any> {
        if let TaskPoll::Pending { fut } = self.as_mut().project() {
            let val = ready!(fut.poll(cx));
            self.set(TaskImpl::Ready { output: Some(val) });
        }
        let TaskPoll::Ready { output } = self.project() else {
            unreachable!()
        };
        // SAFETY: It's okay to return a non-`Pin` reference since the `output`
        // field is not structurally pinned.
        Poll::Ready(output)
    }
}
