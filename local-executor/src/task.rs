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
    pub(crate) fn new<Ex>(executor: Ex, fut: impl 'static + Future<Output = T>) -> Self
    where
        T: 'static,
        Ex: ExecutorHandle,
    {
        let task = WakeableTaskImpl {
            task: RefCell::new(TaskImpl::Pending { fut, waker: None }),
            executor,
        };
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
        // This `Future` will remain pending until the corresponding task is
        // ready and wake it.
        let mut output = Poll::Pending;
        self.inner.0.as_ref().read(cx.local_waker(), &mut output);
        output
    }
}

#[derive(Clone)]
pub(crate) struct TaskRef(Pin<Rc<dyn WakeableTask>>);

impl TaskRef {
    pub(crate) fn poll_wakeable(&self) -> Poll<()> {
        use std::task::{ContextBuilder, Waker};
        let waker = self.0.clone().waker();
        let mut cx = ContextBuilder::from_waker(Waker::noop())
            .local_waker(&waker)
            .build();
        self.0.as_ref().poll(&mut cx)
    }
}

trait WakeableTask {
    fn abort(self: Pin<&Self>);
    fn poll(self: Pin<&Self>, cx: &mut Context) -> Poll<()>;
    fn read(self: Pin<&Self>, waker: &LocalWaker, output: &mut dyn Any);
    fn waker(self: Pin<Rc<Self>>) -> LocalWaker;
}

struct WakeableTaskImpl<T, Ex> {
    task: RefCell<T>,
    executor: Ex,
}

impl<T, Ex> WakeableTaskImpl<T, Ex> {
    fn exclusive_access(self: Pin<&Self>) -> Pin<RefMut<T>> {
        // SAFETY: This is a projection from `Pin<&RefCell>` to `Pin<RefMut>`.
        // It's safe because this method is the only way to grant access to the
        // underlying value, and the returned pointers are always pinned.
        unsafe { Pin::new_unchecked(self.get_ref().task.borrow_mut()) }
    }
}

impl<T, Ex> WakeableTask for WakeableTaskImpl<T, Ex>
where
    T: AnyTask,
    Ex: ExecutorHandle,
{
    fn abort(self: Pin<&Self>) {
        self.exclusive_access().as_mut().abort()
    }
    fn poll(self: Pin<&Self>, cx: &mut Context) -> Poll<()> {
        self.exclusive_access().as_mut().poll(cx)
    }
    fn read(self: Pin<&Self>, waker: &LocalWaker, output: &mut dyn Any) {
        self.exclusive_access().as_mut().read(waker, output)
    }
    fn waker(self: Pin<Rc<Self>>) -> LocalWaker {
        // SAFETY: The pointer is temporarily unpinned to satisfy the signature,
        // and then we immediately pin it back inside `LocalWake::wake`.
        LocalWaker::from(unsafe { Pin::into_inner_unchecked(self) })
    }
}

impl<T, Ex> LocalWake for WakeableTaskImpl<T, Ex>
where
    T: AnyTask,
    Ex: ExecutorHandle,
{
    fn wake(self: Rc<Self>) {
        // SAFETY: See the comments above.
        self.executor
            .get()
            .wake(TaskRef(unsafe { Pin::new_unchecked(self) }))
    }
}

trait AnyTask: 'static {
    fn abort(self: Pin<&mut Self>);
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<()>;
    fn read(self: Pin<&mut Self>, waker: &LocalWaker, output: &mut dyn Any);
}

pin_project_lite::pin_project! {
    #[project = TaskState]
    enum TaskImpl<F: Future> {
        Ready { val: Poll<F::Output> },
        Pending { #[pin] fut: F, waker: Option<LocalWaker> },
    }
}

impl<F> AnyTask for TaskImpl<F>
where
    F: 'static + Future,
{
    fn abort(mut self: Pin<&mut Self>) {
        self.set(Self::Ready { val: Poll::Pending });
    }

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<()> {
        match self.as_mut().project() {
            TaskState::Ready { .. } => Poll::Ready(()),
            TaskState::Pending { fut, waker } => {
                let val = fut.poll(cx);
                if val.is_pending() {
                    return Poll::Pending;
                }
                let waker = waker.take();
                self.set(Self::Ready { val });
                _ = waker.map(LocalWaker::wake);
                Poll::Ready(())
            },
        }
    }

    fn read(mut self: Pin<&mut Self>, waker: &LocalWaker, output: &mut dyn Any) {
        match self.as_mut().project() {
            TaskState::Ready { val } => {
                let output = output.downcast_mut().expect("invalid task state");
                std::mem::swap(val, output)
            },
            TaskState::Pending { waker: Some(w), .. } if !w.will_wake(waker) => *w = waker.clone(),
            TaskState::Pending { waker: w, .. } => *w = Some(waker.clone()),
        }
    }
}
