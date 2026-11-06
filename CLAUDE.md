# CLAUDE.md

本文件给在本地环境接手开发的编码代理（Claude Code 等）。先读本文件，再读 `docs/design-spec.md`，需要了解决策依据时读 `docs/session-notes.md`。

## 项目

产品名 **Smart Library**（简写 `smart-library`，用于 crate 名、可执行文件名、目录名等），本地知识库桌面软件。核心是对《大藏经》等大量文本的快速、精确搜索，并可扩展到本地目录、Obsidian、Wikipedia 离线包和自维护笔记。技术路线：Tauri 2 + Rust + Tantivy + SQLite，仅桌面端。

## 与用户协作

- 用**中文**回复。
- 用户熟悉 Java，Rust 是新手。解释 Rust 特有概念（所有权、借用、trait 对象）时可以对照 Java。
- 用户使用 Windows，测试数据是 `C:\Devx\smartLib.d\bookcase.zip`（CBETA 官方数据包 2025.Q1，用户另加了两个目录树：`zhibei`「索达吉堪布全集」和 `longzang`「乾隆大藏经」，内容是 `a/` 下的 HTML）。控制台显示中文前先执行 `chcp 65001`。
- 用户有旧项目 cbeta-explorer（Java、Solr、JavaFX）和它的解析库 appxi-cbeta，可作为行为参照，但源码不在本仓库。

## 当前状态

- M0a（读取 bookcase、装配目录与 toc、命令行）已编译通过，单元测试全部通过，并在 `C:\Devx\smartLib.d\bookcase.zip`（2025.Q1）上跑过 `info/tree/find/book/check`，打开加 `check` 约 1.5 秒。
- `check` 的结论已写进规范附录 A.1、A.3、A.4：zip 条目名 GBK、一卷分两册、LC 和 Y 两套未登记藏经、`a/` 与 `help/` 叶子。
- M0b（卷 XML 抽取，`core/src/volume.rs`）已完成：全部 21820 卷解析无失败，约 1.5 ms/卷，与 `tools/ref_extract.py` 逐卷对比全部一致。规则变化见规范附录 A.6（跳过 `cb:mulu`、缺字只占一个字符、更多编者符号不计入位置）。
- 下一步是 M0c。

## 下一步

1. **M0c**：自定义分词器（text_aio 加单字），按卷建 Tantivy 索引，命令行搜索，输出命中卷、行号和行内位置，支持宽松匹配与精确短语。纯文本与行号映射来自 `Bookcase::volume_text`。

改动抽取规则时：先改 `tools/ref_extract.py` 和 `core/src/volume.rs` 两边，再跑下面的「抽取回归」两条命令，必须全部一致；锚点一致率不能下降。
2. 待用户决定：`a/` 下的 HTML 叶子何时作为内容节点接入。

已决定：LC、Y 两套未登记藏经**暂不收录**。菜单里有、但取不到真实内容的条目保留在树里，解析时忽略，不报错（规范附录 A.4）。

## 代码约定

- `core` 不依赖界面或 Tauri；适配器不依赖 Tantivy。
- 只读仓库永不修改源数据；所有衍生数据（索引）都必须能删除后重建。
- 先让命令行跑通，再接界面。
- 每发现一条新的数据规则，先在 `samples/` 里加样本，再写测试，再改代码，最后更新 `docs/design-spec.md`。
- 文档以 `docs/design-spec.md` 为准；改变行为时同步更新。
- 不把 `bookcase.zip` 或索引目录提交进仓库（见 `.gitignore`）。

## 关键决策（详见 session-notes）

- 一卷一个文档，只有「卷」被全文索引；节点模型统一，不区分卷、文章、文件、笔记。
- 每个仓库一个适配器、一个独立 Tantivy 索引。
- 分词沿用旧 Solr 的 text_aio（标准分词、全半角折叠、小写、CJK 二字组），并**同时输出单字**；索引时忽略标点，保留位置映射。
- 默认宽松匹配（二字组出现在同一卷内即可），另提供精确短语。
- 命中位置不存入索引：展示时对当前页的结果实时解析卷 XML，得到行号和行内位置。
- 校勘取 `lem`，夹注保留，缺字按 normalized form 优先。

## M0a/M0b 中已验证或仍未验证的假设

- 已验证：`zip = "2"`（实际 2.6.1）改为 `default-features = false, features = ["deflate"]`，bookcase.zip 只用 deflate 和 stored。
- 已实现：zip 条目名先按 UTF-8，失败按 GBK（`encoding_rs`）。
- 未验证：`nav::clean_nav_title` 是否按括号切分（旧辅助函数没见到，是推测）。
- 未验证：中文数字转换（`卷一`、`一百一十`）的写法与旧实现是否一致。
- 已验证：quick-xml 0.42（名称和文本都是 `&str`，实体引用是单独的 `Event::GeneralRef`）。
- `Bookcase::volumes` 对 toc 中列出但文件不存在的卷保留并标记 `exists = false`，对 catalog 推出的卷则只保留文件存在的；按（卷号，路径）去重，一卷分两册时返回两条。

## 常用命令

```
cargo test -p smart-library-core
cargo run -p smart-library-cli --release -- info  <bookcase.zip>
cargo run -p smart-library-cli --release -- tree  <bookcase.zip> simple --depth 2
cargo run -p smart-library-cli --release -- find  <bookcase.zip> 法華
cargo run -p smart-library-cli --release -- book  <bookcase.zip> T0262
cargo run -p smart-library-cli --release -- check <bookcase.zip>
target\release\smart-library.exe info C:\Devx\smartLib.d\bookcase.zip   # 编译后直接运行
cargo run -p smart-library-cli --release -- text  <bookcase.zip> T0262 1          # 一卷，按原书行输出
cargo run -p smart-library-cli --release -- volumes <bookcase.zip>                # 全部卷：失败数、耗时、段落锚点校验
python tools/ref_extract.py samples/cbeta/T01n0004_001.xml     # 需要 lxml（pip install --user lxml；不要加 python -I）

# 抽取回归：Rust 与 Python 逐卷对比（--every 20 约 5 秒；不加 --every 是全部，约 2 GB 临时文件）
target\release\smart-library.exe volumes <bookcase.zip> XML/ --every 20 --out <临时目录>
python tools/diff_extract.py <bookcase.zip> <临时目录>
```
