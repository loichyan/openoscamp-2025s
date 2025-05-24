# 🏕️ 2025 春夏季开源操作系统训练营

SpiralOS 是一个学习性的迷你操作系统，当前的实现主要参考了 [rCore 教程](https://github.com/rcore-os/rCore-Tutorial-Book-v3)，现阶段功能相当不完善，不过相较于 rCore，SpiralOS 有几点不同：

1. SpiralOS 是单 CPU 核心、单内核栈的，即所有的用户任务共享同一个内核栈；
2. SpiralOS 实现了受 [fast-trap](https://github.com/rustsbi/fast-trap) 启发的两阶段陷入处理．

## ⚖️ 许可

本仓库所包含之代码及文档，如无特殊说明，均以下列任意许可授权：

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

[^1]: <https://opencamp.cn/os2edu/camp/2025spring>
