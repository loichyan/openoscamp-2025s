use crate::driver::{DriverHandle, OpId};
use alloc::boxed::Box;
use core::any::Any;
use core::pin::Pin;
use core::task::{Context, LocalWaker, Poll};

pub(crate) enum Lifecycle<T> {
    Submitted,
    Waiting(LocalWaker),
    Completed(T),
    Cancelled(#[allow(dead_code)] Cancellation),
}

pub struct Cancellation(#[allow(dead_code)] Option<Box<dyn Any>>);

impl Cancellation {
    pub const fn noop() -> Self {
        Self(None)
    }

    pub fn recycle<T: 'static>(resource: T) -> Self {
        Self(Some(Box::new(resource)))
    }
}

/// # Safety
///
/// All submitted resources must be recycled.
pub unsafe trait Completable: 'static + Unpin {
    type Output;
    type Payload;

    /// Completes this operation with the received payload.
    fn complete(self, payload: Self::Payload) -> Self::Output;

    /// Cancels this operation.
    fn cancel(self) -> Cancellation;
}

pub struct Op<T, D>
where
    T: Completable,
    D: DriverHandle,
{
    driver: D,
    id: OpId,
    data: Option<T>,
}

impl<T, D> Op<T, D>
where
    T: Completable,
    D: DriverHandle,
{
    pub fn new(driver: D, id: OpId, data: T) -> Self {
        Self {
            driver,
            id,
            data: Some(data),
        }
    }
}

impl<T, D> Future for Op<T, D>
where
    T: Completable,
    D: DriverHandle<Payload = T::Payload>,
{
    type Output = T::Output;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.driver.get().poll(self.id, cx).map(|p| {
            self.data
                .take()
                .expect("invalid operation state")
                .complete(p)
        })
    }
}

impl<T, D> Drop for Op<T, D>
where
    T: Completable,
    D: DriverHandle,
{
    fn drop(&mut self) {
        self.driver.get().remove(self.id, || {
            self.data.take().expect("invalid operation state").cancel()
        });
    }
}
