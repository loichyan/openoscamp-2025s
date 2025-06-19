此模块用于管理共享的内存资源．

请求方通过指针将资源提交给响应方可以达到 zero-copy 的高效数据传递．但在异步上下文中，提前终止一个 [`Future`] 可能导致意外访问过期资源，如下所示，

```rust,ignore
fn read_it(path: &str) {
    let mut buf = [0; 32];
    let mut fut = request_read(path, &mut buf);
    if poll(&mut fut).is_ready() {
        println!("read: {:?}", buf);
    } else {
        cancel(fut);
        //     ^ 尽管我们在此处取消了请求，这并不意味着响应方立即就能停止对该请求的处理
    }
    // <- 当前函数返回，也就意味着 buf 变成了无效的内存，但响应方此后仍可能对它进行写入
}
```

为应对此问题，evering 提供了灵活的资源管理机制．

## 资源

[`Resource`] 和 [`ResourceMut`] 分别定义了只读与可写的资源，资源通常是被分配在内存堆上的数据．二者都要求实现者返回稳定的指针，即不随资源所有权的转移而变化的指针，例如 [`Vec`] 和 [`String`]．

## 基于所有权借用的模型

**定义**：

1. 资源的所有权总是归请求方．
2. 资源被提交给响应方时，所有权通过连接被临时借出．
3. 对于只读资源，借用是共享的．
4. 对于可变资源，借用是独占的．
5. 当资源对应的操作完成时，归还借用．
6. 当系统中不存在借用时，请求方可以选择回收资源．

此模型类比于 Rust 中的引用 `&T` 和可变引用 `&mut T` 的概念．在 evering 中，可以通过 [`Completable::cancel`] 来定义需要回收的资源．在一个操作的生命周期结束时，evering 会自动回收其占用的资源．

以下是一个简单的示例，

```rust
# use evering::driver::*;
# use evering::op::*;
# use std::any::Any;
# use std::rc::{Rc, Weak};
# type DriverHandle = Weak<Driver<Box<dyn Any>>>;
# fn spawn_task<T>(_: T) {}
# let drv = Rc::new(Driver::<Box<dyn Any>>::new());
# let handle = Rc::downgrade(&drv);
# let make_resource = || Box::new(()) as Box<dyn Any>;
# let make_op = |id: OpId, data: Request| Op::new(handle.clone(), id, data);
# let send_request = |_: *const dyn Any| {};
# let recv_response = || Response { payload: Box::new(()) };
struct Request {
    resource: Box<dyn Any>,
    // ... 一个操作可能占用多种资源
}
struct Response {
    payload: Box<dyn Any>,
}
unsafe impl Completable for Request {
    type Output = Box<dyn Any>;
    type Driver = DriverHandle;
    fn complete(self, _: &Self::Driver, payload: Box<dyn Any>) -> Box<dyn Any> {
        payload
    }
    fn cancel(self, _: &Self::Driver) -> Cancellation {
        Cancellation::recycle(self.resource)
        //                    ^ 一般来说，所有按指针传递的资源都需要回收
    }
}

let resource = make_resource();
send_request(&raw const *resource); // <- 将该请求发送给响应端

let id = drv.submit();
let op = make_op(id, Request { resource });
spawn_task(op); // <- 后台轮询 Op，继续处理其它工作

// ...

let data = recv_response(); // <-直到接收响应，随后完成该操作
drv.complete(id, data.payload).ok();
//  ^ 被占用的资源已通过 Cancellation 自动回收，故此处可以忽略返回的错误
```

**优点**:

1. [`Cancellation`] 可以支持任意类型的资源．
2. 资源管理完全由请求方控制，因此适用于各种场景．

**缺点**:

1. [`Cancellation`] 的底层实现依赖于动态派发和内存分配，因此会引入额外的开销．不过，由于实际场景中，取消操作是一个概率相对较小的事件，所以此开销几乎可以忽略．
2. 请求方必须确保全部已提交的资源在取消时被回收掉．不过，比起下文所述模型，这相对不容易出错．

此外，在实现此模型时，evering 参考了 [`tokio-uring`](https://github.com/tokio-rs/tokio-uring) 和 [`ringbahn`](https://github.com/ringbahn/ringbahn) 的相关设计．

## 基于所有权转移的模型

**定义**:

1. 资源的所有权归属使用方．
2. 资源的所有权通过连接在通信双方之间转移．
3. 一方确认资源不需要再被转移时，它可以选择回收资源．

此模型类比于 Rust 中的移动 `move` 语义．evering 并没有直接实现此模型，相反，这种机制依赖与通信双方的配合，如下所示，

```rust
# use evering::driver::*;
# use evering::op::*;
# use std::any::Any;
# use std::rc::{Rc, Weak};
# type DriverHandle = Weak<Driver<()>>;
# fn spawn_task<T>(_: T) {}
# let drv = Rc::new(Driver::<()>::new());
# let handle = Rc::downgrade(&drv);
# let make_resource = || Box::leak(Box::new(())) as *mut dyn Any;
# let make_op = |id: OpId, data: Request| Op::new(handle.clone(), id, data);
# let send_request = |_: *mut dyn Any| {};
# let recv_response = || Response { resource: Box::leak(Box::new(())) };
struct Request {
    resource: *mut dyn Any,
    //        ^ 请求方不应独占资源所有权
}
struct Response {
    resource: *mut dyn Any, // <- 写入资源后，响应方需要返还所有权
}
unsafe impl Completable for Request {
    type Output = ();
    type Driver = DriverHandle;
    fn complete(self, _: &Self::Driver, _: ()) {
        // SAFETY:
        // 此时系统中不再有对该资源的访问，
        // 1. 所有权已由响应方转移给我们．
        // 2. 它对应的 Op 已完成．
        // 因此可以安全的 drop 它．
        unsafe { self.resource.drop_in_place() }
    }
    fn cancel(self, _: &Self::Driver) -> Cancellation {
        Cancellation::noop()
        //            ^ 这里不需要回收任何资源
    }
}

let resource = make_resource();
send_request(resource); // <- 响应方会得到该资源的所有权，并对其进行更新

let id = drv.submit();
let op = make_op(id, Request { resource });
spawn_task(op); // <- 后台轮询 Op，继续处理其它工作

// ...

let data = recv_response(); // <- 接收响应随后完成该操作
if drv.complete(id, ()).is_err() {
    // SAFETY:
    // 和上述类似，由于该资源对应的 Op 已被取消，系统中不再有对它的访问．
    unsafe { data.resource.drop_in_place() }
}
```

**优点**:

1. 避免了引入 [`Cancellation`] 导致的额外开销．
2. 资源所有权的归属配合 Rust 的 `move` 语义更符合直觉．

**缺点**:

1. 通信双方需要提前协商资源所有权的归属．
2. 通信双方必须谨慎的、手动的控制资源的释放．

此外，我们也可以利用 [`Driver`] 提供的 extension 机制来克服 *(1)* 所述的缺点，如下所示，

```rust
# use evering::driver::*;
# use evering::op::*;
# use std::any::Any;
# use std::rc::{Rc, Weak};
# type DriverHandle = Weak<Driver<(), *mut dyn Any>>;
# fn spawn_task<T>(_: T) {}
# let drv = Rc::new(Driver::<(), *mut dyn Any>::new());
# let handle = Rc::downgrade(&drv);
# let make_resource = || Box::leak(Box::new(())) as *mut dyn Any;
# let make_op = |id: OpId, data: Request| Op::new(handle.clone(), id, data);
# let send_request = |_: *mut dyn Any| {};
# let recv_response = || Response {};
struct Request {
    // resource: *mut dyn Any,
}
struct Response {
    // resource: *mut dyn Any,
}
unsafe impl Completable for Request {
    type Output = ();
    type Driver = DriverHandle;
    fn complete(self, _: &Self::Driver, _: ()) {
        unreachable!() // <- 此函数不会被调用，如果实现了 complete_ext
    }
    fn complete_ext(self, _: &Self::Driver, _: (), resource: *mut dyn Any) {
        // SAFETY: 同上所述
        unsafe { resource.drop_in_place() }
    }
    fn cancel(self, _: &Self::Driver) -> Cancellation {
        Cancellation::noop()
    }
}

let resource = make_resource();
send_request(resource);

let id = drv.submit_ext(resource);
//                      ^ 将资源作为 extension 存放在 Driver 中
let op = make_op(id, Request {}); // <- 不再需要储存资源指针
spawn_task(op);

// ...

let data = recv_response();
if let Err((_, resource)) = drv.complete_ext(id, ()) {
    //         ^ Driver 将一直保存该资源，直到对应操作的生命周期结束
    // SAFETY: 同上所述
    unsafe { resource.drop_in_place() }
}
```

[`Cancellation`]: crate::op::Cancellation
[`Completable::cancel`]: crate::op::Completable::cancel
[`Driver`]: crate::driver::Driver
[`Future`]: core::future::Future
[`String`]: alloc::string::String
[`Vec`]: alloc::vec::Vec
