use crate::executor::ExecutorHandle;
use std::any::Any;
use std::cell::{RefCell, RefMut};
use std::marker::PhantomData;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, LocalWake, LocalWaker, Poll};

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
        let task = WakeableTaskImpl(RefCell::new(TaskImpl::Pending { fut }));
        Self {
            inner: TaskRef(Rc::pin(task)),
            marker: PhantomData,
        }
    }

    pub(crate) fn inner(&self) -> TaskRef {
        self.inner.clone()
    }

    pub fn abort(self) {
        self.inner.0.as_ref().abort();
    }
}

impl<T: 'static> Future for Task<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let mut output = Poll::Pending::<T>;
        self.inner.0.as_ref().poll_read(cx, Some(&mut output));
        output
    }
}

#[derive(Clone)]
pub(crate) struct TaskRef(Pin<Rc<dyn WakeableTask>>);

impl TaskRef {
    pub(crate) fn poll_wakeable(&self) {
        use std::task::{ContextBuilder, Waker};
        let waker = self.0.clone().waker();
        let mut cx = ContextBuilder::from_waker(Waker::noop())
            .local_waker(&waker)
            .build();
        self.0.as_ref().poll_read(&mut cx, None);
    }
}

trait WakeableTask {
    fn abort(self: Pin<&Self>);
    fn poll_read(self: Pin<&Self>, cx: &mut Context, output: Option<&mut dyn Any>);
    fn waker(self: Pin<Rc<Self>>) -> LocalWaker;
}

struct WakeableTaskImpl<T>(RefCell<T>);

impl<T> WakeableTaskImpl<T> {
    fn exclusive_access(self: Pin<&Self>) -> Pin<RefMut<T>> {
        // SAFETY: This is a projection from `Pin<&RefCell>` to `Pin<RefMut>`.
        // It's safe because this method is the only way to grant access to the
        // underlying value, and the returned pointers are always pinned.
        unsafe { Pin::new_unchecked(self.get_ref().0.borrow_mut()) }
    }
}

impl<T> WakeableTask for WakeableTaskImpl<T>
where
    T: AnyTask,
{
    fn abort(self: Pin<&Self>) {
        self.exclusive_access().as_mut().abort();
    }
    fn poll_read(self: Pin<&Self>, cx: &mut Context, output: Option<&mut dyn Any>) {
        self.exclusive_access().as_mut().poll_read(cx, output);
    }
    fn waker(self: Pin<Rc<Self>>) -> LocalWaker {
        // SAFETY: The pointer is temporarily unpinned to satisfy the signature,
        // and then we immediately pin it back inside `LocalWake::wake`.
        LocalWaker::from(unsafe { Pin::into_inner_unchecked(self) })
    }
}

impl<T: AnyTask> LocalWake for WakeableTaskImpl<T> {
    fn wake(self: Rc<Self>) {
        // SAFETY: See the comments above.
        ExecutorHandle::wake(TaskRef(unsafe { Pin::new_unchecked(self) }))
    }
}

trait AnyTask: 'static {
    fn abort(self: Pin<&mut Self>);
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context, output: Option<&mut dyn Any>);
}

pin_project_lite::pin_project! {
    #[project = TaskState]
    enum TaskImpl<F> {
        Aborted,
        Pending { #[pin] fut: F },
    }
}

impl<F> AnyTask for TaskImpl<F>
where
    F: 'static + Future,
{
    fn abort(mut self: Pin<&mut Self>) {
        self.set(Self::Aborted);
    }

    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context, output: Option<&mut dyn Any>) {
        let TaskState::Pending { fut } = self.as_mut().project() else {
            debug_assert!(output.is_none(), "invalid task state");
            return;
        };
        let Poll::Ready(val) = fut.poll(cx) else {
            return;
        };
        let Some(output) = output else {
            return;
        };
        *output.downcast_mut().expect("invalid task state") = Poll::Ready(val);
    }
}
