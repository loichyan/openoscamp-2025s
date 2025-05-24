# 🏕️ 2025 春夏季开源操作系统训练营

使用 async Rust 实现的简易爬虫客户端，目前的实现相当简单：抓取网页然后从 HTML 文档中解析链接，再爬取，如此重复．

基本用法：

```sh
RUST_LOG=info cargo run --release -- -d data --max-depth=2 --max-rate=2 --url=...
```

## ⚖️ 许可

本仓库所包含之代码及文档，如无特殊说明，均以下列任意许可授权：

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

[^1]: <https://opencamp.cn/os2edu/camp/2025spring>
