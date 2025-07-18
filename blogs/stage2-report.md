# 2025 春夏季开源操作系统训练营第二阶段总结报告

## 学习历程

因为并非计算机专业出身，所以一直都有系统学习计算机底层原理的念头，趁着备研把理论性的知识学了一些．虽然现阶段只看了计算机组成原理，不过，早便萌生了自己动手去实践那些理论技巧的想法．去年秋冬季的夏令营也有报名，但由于时间安排仓促混乱，只得作罢．今年，虽然学习安排亦比较紧张，不过，想着动手得来的知识要比苦读书本记忆的更为深刻．

在第一阶段中，因为本身就有一些 Rust 开发经验，所以总体上是复习了一遍语言基础，前后并没有占用多少时间．第二阶段开始的时间稍晚，在 4 月下旬才腾出时间来专门研读 rCore 教程，也算是为了接下来学习操作系统做铺垫吧．一直拖沓的月底才把 rCore 教程 V3[^1] 看完，五月初到现在差不多 4 天的时间完成了各章节的实验题目．

## 主要收获

在阅读 rCore 教程的时候，始终有两个问题一直困扰我，在 rCore 的实现中，

1. 内核虚拟内存空间和用户虚存被映射到了相同的地址段，这导致除了访问用户指针时需要先映射到实际的物理内存，稍有遗漏便可能导致访存异常，能否使用一个页表呢？
2. 每个任务都有两个栈，即内核栈和用户栈，并且在调度任务时需要反复地在不同内核栈之间轮替，能否使用一个单独的栈解决问题呢？

### 单页表虚拟内存

单页表的实现在教程中有所提及，关于为何没有使用，文中给出的理由是单页表会导致熔断攻击．如果要避免熔断攻击，那么用户态就一定不能包含内核空间的页表项，这一点很容易想到．再稍加思考，只需要将地址空间一分为二，高位和低位分别留给内核和用户，然后再维护两个页表，其中一个只包含用户程序和必要的内核程序即可．后来，进一步研究了 Linux 的 PTI[^2] 机制后，发现基本的思路一致．并且从中了解到，在实现细节上，可以将两个页表放在物理内存的两个连续页面上，这样只需要若干寄存器运算就能实现换表．

起初我一直认为单页表无疑是比双页表更简洁方便的方案．后来，在落实时才发现，实现单页表的一大难点在于：内核被映射到虚拟内存的高位（比如 0xffffffc080200000），但实际上被加载到物理内存的低位运行（比如 0x80200000）．这便到导致了内核链接时的基址和运行时的基址不一致，从而可能使得某些绝对寻址的访存错误，进一步引发难以察觉的 BUG．故，需要在内核启动时，需要手写一小段仅使用相对寻址的汇编代码来构造一个最小可用的内核页表，然后再进行更为细致地初始化流程．

在这个过程中，本打算参考 ArceOS[^3] 的实现，但此内核中直接在若干简短的 Rust 函数中完成了初始化．猜测应该是编译成了位置无关的代码，但总感觉这样有些”过度“依赖于编译器．因此，经过一番探索之后，用大约 50 行汇编代码完成了最小内核页表的初始化工作．这个过程是比较困难的，因为整个系统还没有启动，非常难以调试．不过也因此深化了对 GDB 等调试工具的理解和使用．

此外，在更进一步地思考后，发现每个程序的内核页表都是该程序初始化时的全局内核页表的一个拷贝，如果后续内核页表更新，可能会导致访存的不一致．对于此问题，最简单的办法就是预先分配好三级页表的全部项，这样后续就完全不必担心同步的问题．当然，这不可避免的占用了少量额外内存（更确切地，2MB），但这个“少量”对于嵌入式设备来说并非可接受的开销．因此，最好的方案似乎还是按需同步，考虑到内核页表不会变化太频繁，这应该是一个合理的选择．不过，最终该如何漂亮地解决这个问题，还需要进一步的调研．

### 单内核栈系统

刚开始学习多任务时，就一直纠结单内核栈该怎么实现．后来学习进程管理机制时，突然意识到，如果换一个角度看待用户程序：对操作系统来说，执行用户程序相当于调用一个函数，只不过，函数的返回依赖于陷入机制．这一点和用户角度很像，即不发生异常时，大多数时候，操作系统对于用户程序就是一个提供了许多系统调用的函数库．用户程序可以在一个栈上“不间断地”执行（陷入处理对用户透明），那么操作系统肯定也能实现类似的机制．

对于进一步的细节，相当于将 rCore 的上下文切换和用户执行态恢复整合一起：每次需要执行用户程序时，将当前的内核运行状态保存；而在处理用户陷入时，保存过用户的执行状态后，紧接着便加载先前保存的内核状态．整体上看，相当于内核把用户视为函数调用了一次，而在它返回后，内核便可以着手进行调度或者处理用户请求．

这样，一个最显著的优点便是使得内核的调度更加直观：从始自终，内核在一个循环中不断地“call”用户程序．并且，可以大幅减少全局变量的使用，更容易利用 Rust 所有权模型的优势，也有利好内核实现复杂的调度算法．

### 两阶段陷入处理

上述单内核栈的方案也引发了一个新问题：每次发生陷入都要保存大量寄存器，包括全部的用户通用寄存器、CSR 以及一部分调用上下文；而多内核栈的方案中，不发生调度时，相比可以减少约 2/3 的寄存器存取操作．因此，自然而然地，需要找到一种方法来减少上下文的保存操作．

由陷入机制入手，从本质上讲，陷入处理实际上是在任意位置插入一条函数调用，而各种处理器架构均定义了汇编层面的调用约定．既然陷入处理相当于函数调用，那么在陷入处理的入点，只需要保存调用者（caller-saved）寄存器和若干其它寄存器（CSR、tp、sp 等）即可．后来，偶然发现了 rCore 维护者提出的 fast-trap[^4] 快速陷入处理机制，这是一个相当通用（但有些复杂）的多阶段陷入处理机制．

深入研究之后，发现似乎两个阶段的陷入处理机制就能满足绝大部分的需求：

1. 第一阶段，只需要保存约一半的寄存器（如上文所述），主要用于处理各种异常（包括内核和用户）以及一部分不需要切换上下文的系统调用．处理内核陷入仅需要此阶段即可．
2. 第二阶段，保存完整的用户寄存器和内核调用上下文，沿用前文单内核栈方案的陷入处理机制．

在经过大量试错和调整之后，最终在不需要额外的访存操作的情况下，实现了一个相对通用的两阶段陷入处理方案．实现时，更进一步学习了许多 GDB 的调试技巧以及 Rust 声明宏的使用技巧．

### 总结

以上便是第二阶段中的主要收获，此外，关于文件系统和各种同步原语的实现，也偶有一些“灵光一现”，不过限于时间不足，并没来得及实践．对于下一阶段，打算集中精力攻克第三个选题，即基于 Rust 语言异步机制的内核改造．因为，此前一直对 Rust 的异步实现“耿耿于怀”，借此机会，可以更深入理解其工作原理．

<!-- dprint-ignore-start -->
[^1]: <https://rcore-os.cn/rCore-Tutorial-Book-v3/>
[^2]: <https://www.kernel.org/doc/html/v6.1/x86/pti.html>
[^3]: <https://github.com/arceos-org/arceos/>
[^4]: <https://github.com/rustsbi/fast-trap/>
