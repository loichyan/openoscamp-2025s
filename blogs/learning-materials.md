# 学习资料搜集

零零散散记录了学习过程中阅读的一些资料：

- Linux io_uring 基本原理 ([链接](https://arthurchiao.art/blog/intro-to-io-uring-zh/))
  - 文中提到可以利用 io_uring 将全部 syscall 改造成异步，如此大减少了系统调用的上下文切换
- 关于 io_uring 的论文 ([链接](https://kernel.dk/io_uring.pdf), [翻译](https://icebergu.com/archives/linux-iouring))
  - 较详细的介绍了 io_uring 的总体设计和工作原理
- Pre-RFC interrupt_calling_conventions ([链接](https://github.com/phil-opp/rfcs/blob/interrupt-calling-conventions/text/0000-interrupt-calling-conventions.md))
  - RFC 提出一种通用的 abi 用来处理硬件中断，可以让编译器来处理中断的上下文保存
  - 此外一些平台已有对应的 unstable feature
    - [avr_interrupt](https://doc.rust-lang.org/nightly/unstable-book/language-features/abi-avr-interrupt.html)
    - [msp430_interrupt](https://doc.rust-lang.org/nightly/unstable-book/language-features/abi-msp430-interrupt.html)
    - [riscv_interrupt](https://doc.rust-lang.org/nightly/unstable-book/language-features/abi-riscv-interrupt.html)
    - [x86_interrupt](https://doc.rust-lang.org/nightly/unstable-book/language-features/abi-x86-interrupt.html)
- Rust Atomics and Locks ([链接](https://marabos.nl/atomics/))
  - 书中由浅入深的介绍了 Rust 中原子类型的工作原理和硬件细节
