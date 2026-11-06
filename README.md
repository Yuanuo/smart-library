# Smart Library

Smart Library（简写 `smart-library`）是一个轻量但搜索强大的本地知识库桌面软件。首要数据源是 CBETA 的《大藏经》（`bookcase.zip`），之后扩展到本地目录、Obsidian、Wikipedia 离线包和自维护笔记。

技术路线：Tauri 2 + Rust + Tantivy + SQLite，仅桌面端。

## 状态

- 设计规范：`docs/design-spec.md`（已整理）。
- M0a：读取 bookcase、装配目录与每部经的 toc、命令行检查工具。已完成，已在真实数据上验证。
- M0b：卷 XML 抽取（纯文本、行号映射、段落锚点）。已完成，全部 21820 卷验证。
- 后续：M0c 索引与搜索、M1 界面。见规范第 11 节。

## 目录

```
CLAUDE.md             给编码代理的接手说明（先读）
docs/design-spec.md   设计规范与 CBETA 格式附录
docs/session-notes.md 设计会话纪要：决策、依据、验证结果
docs/reference/       旧项目 cbeta-explorer 的截图，bookcase.zip 目录截图
core/                 smart-library-core：解析与装配（Rust 库）
cli/                  smart-library-cli：命令行检查工具（可执行文件 smart-library）
samples/cbeta/        真实数据样本与黄金输出
data/bookinfo.json    人工整理的经籍元数据（典类、朝代、作译者），4826 条
tools/                ref_extract.py（卷 XML 参考提取器）、diff_extract.py（与 Rust 逐卷对比）、convert_bookinfo.py
```

## 开始

```
cargo test -p smart-library-core
cargo run -p smart-library-cli --release -- check C:\Devx\smartLib.d\bookcase.zip
```

Windows 控制台先执行 `chcp 65001` 以正确显示中文。`bookcase.zip` 不在仓库中，请放在仓库之外。

## 初始化为 git 仓库

```
git init
git add .
git commit -m "Initial import: design spec, M0a core, samples"
```
