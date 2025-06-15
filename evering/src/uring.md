此模块提供了用于建立通信连接的数据结构．

[`Uring`] 定义了用于发送和接收消息的端口，而 [`Builder`] 则用于构造一个通信连接的两端．[`UringA`] 和 [`UringB`] 是成对的两个通信端口，二者在连接中的地位是对等的．

## 非对等通信

[`Sender`] 和 [`Receiver`] 分别是 A、B 两个端口的别名，用于在语义上区分在通信中的两个不同角色．以下演示了如何建立非对等通信连接，

```rust
# use evering::uring::*;
# use std::sync::atomic::AtomicBool;
let items = vec![1, 2, 3, 4, 5];
// 初始化发送端 tx 和接收端 rx
let (mut tx, mut rx) = Builder::<i32, i32>::new().build();
std::thread::scope(|cx| {
    cx.spawn(|| {
        // 将请求批量发送至接收端
        tx.send_bulk(items.iter().copied());
        let mut r = vec![];
        // 接收并处理响应结果
        while r.len() != items.len() {
            r.extend(tx.recv_bulk().map(|i| i >> 1));
            std::thread::yield_now();
        }
        assert_eq!(r, items);
    });
    cx.spawn(|| {
        let mut n = 0;
        loop {
            // 接受请求，处理之，并将响应送回
            while let Some(i) = rx.recv() {
                rx.send(i << 1).unwrap();
                n += 1;
            }
            if n == items.len() {
                break;
            }
            std::thread::yield_now();
        }
    });
});
```

## 对等通信

尽管 [`UringA`] 和 [`UringB`] 可以用来建立对等通信，但如果一方的角色无法在编译期间确定，二者就无法应对了．[`UringEither`] 允许在运行时决定某一方的角色，但它要求通信双方的消息类型是一致的．以下演示了如何用它建立对等通信，

```rust
# use evering::uring::*;
# use std::sync::atomic::AtomicBool;
let items = vec![1, 2, 3, 4, 5];
let worker = |mut p: UringEither<i32>| {
    //               ^ UringA 和 UringB 是两个不同的类型
    p.send_bulk(items.iter().copied());
    let mut r = vec![];
    while r.len() != items.len() {
        r.extend(p.recv_bulk());
        std::thread::yield_now();
    }
    assert_eq!(r, items);
};
let (pa, pb) = Builder::<i32, i32>::new().build();
std::thread::scope(|cx| {
    cx.spawn(|| worker(UringEither::A(pa)));
    cx.spawn(|| worker(UringEither::B(pb)));
});
```

## 内存共享

在不同进程之间通过共享内存来建立连接时，分配给 [`Uring`] 的内存对双方来说可能是不同的地址．这时就需要通过 [`RawUring`] 来手动处理这一差异．[`Uring`] 可以和 [`RawUring`] 相互转换，而后者暴露了必要的接口以便控制底层的内存细节．

以下展示了如何手动管理 [`Uring`] 的内存分配，

```rust
# use evering::uring::*;
# use std::alloc::Layout;
# use std::ptr::NonNull;
# use std::sync::atomic::{Ordering, fence};
# fn alloc_header(h: Header<()>) -> NonNull<Header<()>> { NonNull::from(Box::leak(Box::new(h))) }
# fn alloc_buffer(size: usize) -> NonNull<i32> {
#     let layout= Layout::array::<i32>(size).unwrap();
#     unsafe { NonNull::new(std::alloc::alloc(layout)).unwrap().cast() }
# }
# fn dealloc_uring(_: RawUring<i32, i32>) {}
let header = Builder::<i32, i32>::new().build_header();
//                                      ^ 仅初始化 Header
// 随后手动分配内存，也可以从已分配的内存中构造 RawUring
let buf_a = alloc_buffer(header.size_a());
let buf_b = alloc_buffer(header.size_b());
let header = alloc_header(header);
let build_raw = || { // <- RawUring 是非 Clone 的
    let mut raw = RawUring::dangling();
    raw.header = header;
    raw.buf_a = buf_a;
    raw.buf_b = buf_b;
    raw
};
let (pa, pb);
// SAFETY: 我们可以确保内存是有效的
unsafe {
    pa = UringA::from_raw(build_raw());
    pb = UringB::from_raw(build_raw());
}
assert!(pa.is_connected() && pb.is_connected());
// 对于自定义分配的内存，我们必须手动释放
assert!(pa.dispose_raw().is_err());
//                       ^ 当任一端存活时，内存不会被释放
let raw = pb.dispose_raw().unwrap();
//                         ^ 此时可以安全的释放内存
dealloc_uring(raw);
```
