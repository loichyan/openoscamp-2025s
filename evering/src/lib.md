> **evering** is not **ever** a **ring**

evering 是一个受 [io_uring](https://en.wikipedia.org/wiki/Io_uring) 启发的 SPSC（Single Producer Single Consumer, 单生产者单消费者）异步通信框架．

## 通信

对于通信双方，至少满足以下条件时才有可能通过 evering 安全的建立连接：

1. 双方有可读写的共享内存区域；
2. 双方可获知对方的存活状态．

常见的场景是两个线程之间的异步通信，但这里的线程不必局限于同一进程，它可以是两个不同的用户进程，也可以是用户线程和内核线程

### 对等性

我们用对等性来描述双方在通信中扮演的角色．以基于 HTTP 的 [C-S 模型](https://en.wikipedia.org/wiki/Client–server_model) 为例，一般来说，它是非对等的通信模型，即客户端只能单向的、主动的向服务端请求资源．而当客户端服务端通过 Websocket 等技术建立全双工连接时，它们之间的通信就是对等的了．对等通信可以兼容非对等通信，反之则不然．

evering 在内部使用两个队列来传递消息，因此可以实现对等通信．[`uring`] 模块提供了建立连接的基础数据结构．

### 操作的生命周期

一方在连接中提交一次操作（operation）称为发起一次请求，发起者是请求方，接收者是响应方．响应是指响应方结束对操作的处理后在连接中通知请求方该操作的完成．一个操作的生命周期自发起请求开始，到答复响应结束．在整个过程中，请求方理应可以安全的取消任何已经提交的操作．[`op`] 模块进一步定义了操作的细节，而 [`driver`] 模块则用于管理操作的生命周期．

## 资源管理

evering 鼓励通过共享内存传递数据，而请求消息则仅用来协商数据的定义和使用．在异步环境下，请求方无法及时通知响应方取消某个请求，这就可能导致意外访问过期资源．以下面的代码为例，

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

这个问题可以被更一般的描述为 `Future` 的终止安全性（Cancellation Safety[^1]）．[`resource`] 模块详细介绍了 evering 提供的几种资源管理模型．

[^1]: <https://docs.rs/tokio/latest/tokio/macro.select.html#cancellation-safety>
