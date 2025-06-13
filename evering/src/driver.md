此模块用于管理操作生命周期．

所有的 [`Op`] 在其生命周期中都存放在 [`Driver`] 之中，[`Driver::submit`] 是它们生命周期的开始，而 [`Driver::complete`] 是生命周期的结束．

[`Op`] 在其生命周期中往往需要与 [`Driver`] 交互，[`DriverHandle`] 定义了如何访问它所在的 [`Driver`]．

以下演示了操作生命周期的基本管理流程，

```rust
# use evering::driver::*;
# use evering::op::*;
# use std::pin::pin;
# use std::rc::{Rc, Weak};
# use std::task::{Context, Waker};
# struct Noop;
# unsafe impl Completable for Noop {
#     type Output = ();
#     type Driver = Weak<Driver<()>>;
#     fn complete(self, _: &Self::Driver, _: ()) {}
#     fn cancel(self, _: &Self::Driver) -> Cancellation { Cancellation::noop() }
# }
# let mut cx = Context::from_waker(Waker::noop());
let drv = Rc::new(Driver::<()>::new());
//        ^ 默认情况下，Weak 实现了 DriverHandle
let id = drv.submit();
//           ^ 提交请求，这里是 Op 生命周期的开始
let mut op = pin!(Op::new(Rc::downgrade(&drv), id, Noop));
//      ^ Op 本身实现了 Future 以便于在异步函数中使用
assert!(op.as_mut().poll(&mut cx).is_pending());
//                                ^ 在响应前 Op 不会 ready
assert!(drv.complete(id, ()).is_ok());
//          ^ 提交响应，这里是 Op 生命周期的结束，同时它所在的 Future 被唤醒
assert!(op.as_mut().poll(&mut cx).is_ready());
//                                ^ 响应后 Op 立即就绪
```

[`Op`]: crate::op::Op
