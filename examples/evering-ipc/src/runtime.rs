use std::cell::RefCell;
use std::mem::ManuallyDrop;
use std::rc::{Rc, Weak};

use evering::driver::OpId;
use evering::op::Completable;
use evering_utils::runtime::ExecutorRef;
use local_executor::Task;

use crate::op::{Rqe, RqeData, Sqe};

type Sender = evering::uring::Sender<Sqe, Rqe>;
type RuntimeInner = evering_utils::runtime::Runtime<RqeData, Sender>;

pub struct Runtime(ManuallyDrop<Rc<RuntimeInner>>);

impl Runtime {
    pub fn new(sender: Sender) -> Self {
        Self(ManuallyDrop::new(Rc::new(RuntimeInner::new(sender))))
    }

    pub fn block_on<T>(&self, fut: impl Future<Output = T>) -> T {
        let _guard = RuntimeHandle::enter(&self.0);
        self.0.block_on(self.run_on_no_guard(fut))
    }

    pub async fn run_on<T>(&self, fut: impl Future<Output = T>) -> T {
        let _guard = RuntimeHandle::enter(&self.0);
        self.run_on_no_guard(fut).await
    }

    async fn run_on_no_guard<T>(&self, fut: impl Future<Output = T>) -> T {
        self.0
            .run_on(|rqe| _ = self.0.driver.complete(rqe.id, rqe.data), fut)
            .await
    }

    pub fn into_uring(mut self) -> Sender {
        let rc = unsafe { ManuallyDrop::take(&mut self.0) };
        std::mem::forget(self);
        Rc::into_inner(rc)
            .unwrap_or_else(|| unreachable!("there should not be other strong references"))
            .into_uring()
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        let rc = unsafe { ManuallyDrop::take(&mut self.0) };
        // Leak the Driver so that no pending resources will expire.
        // TODO: should wait instead?
        if !rc.driver.is_empty() {
            std::mem::forget(rc);
        }
    }
}

thread_local! {
    static CX: RefCell<Weak<RuntimeInner>> = const { RefCell::new(Weak::new()) };
}

pub struct RuntimeHandle;

impl evering_utils::runtime::RuntimeHandle for RuntimeHandle {
    type Payload = RqeData;
    type Uring = Sender;
    type Ref = Rc<RuntimeInner>;
    fn get(&self) -> Self::Ref {
        CX.with_borrow(Weak::upgrade)
            .expect("not inside a valid reactor")
    }
}
impl local_executor::ExecutorHandle for RuntimeHandle {
    type Ref = ExecutorRef<RuntimeHandle>;
    fn get(&self) -> Self::Ref {
        ExecutorRef::new(self)
    }
}
impl evering::driver::DriverHandle for RuntimeHandle {
    type Payload = RqeData;
    type Ext = ();
    type Ref = evering_utils::runtime::DriverRef<RuntimeHandle>;
    fn get(&self) -> Self::Ref {
        evering_utils::runtime::DriverRef::new(self)
    }
}

impl RuntimeHandle {
    fn enter(cx: &Rc<RuntimeInner>) -> impl Drop {
        struct Revert;
        impl Drop for Revert {
            fn drop(&mut self) {
                CX.with_borrow_mut(|d| *d = Weak::new())
            }
        }
        CX.with_borrow_mut(|d| {
            if d.strong_count() != 0 {
                panic!("cannot run within a nested reactor")
            }
            *d = Rc::downgrade(cx)
        });
        Revert
    }

    pub fn spawn<T, F>(fut: F) -> Task<T>
    where
        T: 'static,
        F: 'static + Future<Output = T>,
    {
        RuntimeInner::spawn(Self, fut)
    }

    pub async fn submit<T>(data: T, new_entry: impl FnOnce(OpId, &mut T) -> Sqe) -> T::Output
    where
        T: Completable<Driver = RuntimeHandle>,
    {
        RuntimeInner::submit(Self, data, new_entry).await.await
    }
}
