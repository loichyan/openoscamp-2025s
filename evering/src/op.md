此模块定义了用于异步环境下的操作．

[`Op`] 表示一个提交到 [`Driver`] 的操作，它实现了 [`Future`]，因此可以在异步上下文中使用 `.await` 来等待该操作的返回值．所有的操作都需要实现 [`Completable`]，它描述了如何将接收的响应值转换为对应操作的返回值，以及如何回收该操作已提交的[资源](crate::resource)．

## 操作的取消

当 [`Op`] 被 [`drop`] 时，

1. 如果该操作已完成，那么它对应的 [`OpId`] 和资源将被回收．
2. 否则，[`Completable::cancel`] 被调用，直到其生命周期结束时，它占用的资源才会被回收．

每个 [`Completable`] 必须在被取消时回收全部已提交的资源，否则将会导致 UB．如果某个操作被取消，但它的生命周期一直没有结束，这是安全的，但会导致资源泄露，如下所示，

```rust
# use evering::driver::*;
# use evering::op::*;
# use std::rc::{Rc, Weak};
# struct Noop;
# unsafe impl Completable for Noop {
#     type Output = ();
#     type Driver = Weak<Driver<()>>;
#     fn complete(self, _: &Self::Driver, _: ()) {}
#     fn cancel(self, _: &Self::Driver) -> Cancellation { Cancellation::noop() }
# }
# let drv = Rc::new(Driver::<()>::new());
# let handle = Rc::downgrade(&drv);
let id = drv.submit();
let op = Op::new(handle, id, Noop);
drop(op); // <- 被 drop 时，Op 自动被取消
assert!(drv.contains(id));
//          ^ Op 占用的资源将一直存在
assert!(drv.complete(id, ()).is_err());
//                           ^ 完成被取消的 Op 会返回错误
assert!(!drv.contains(id));
//      ^ 直到生命周期结束它占用的资源才被回收
```

调用 [`Driver::complete`] 时，它会返回指定的操作是否已被取消，因此实现者也可以利用这一点来回收资源．

[`Driver`]: crate::driver::Driver
[`Driver::complete`]: crate::driver::Driver::complete
[`Future`]: core::future::Future
[`OpId`]: crate::op::OpId
[`drop`]: core::ops::Drop
