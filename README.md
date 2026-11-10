# Smart Library

Smart Library（简写 `smart-library`）是一个轻量但搜索强大的本地知识库桌面软件。首要数据源是 CBETA 的《大藏经》（`bookcase.zip`），之后扩展到本地目录、Obsidian、Wikipedia 离线包和自维护笔记。

技术路线：Tauri 2 + Rust + Tantivy + SQLite，仅桌面端。

## 状态

- 设计规范：`docs/design-spec.md`（已整理）。
- M0a：读取 bookcase、装配目录与每部经的 toc、命令行检查工具。已完成，已在真实数据上验证。
- M0b：卷 XML 抽取（纯文本、行号映射、段落锚点）。已完成，全部 21820 卷验证。
- M0c：分词（二字组、单字、繁简互查）、Tantivy 索引、命令行搜索。已完成。
- M1：桌面程序骨架（Tauri 2 + Svelte 5）：目录树、按行阅读、搜索、分面、命中跳转。
- 结构化阅读（段落、偈颂、科判、校注气泡、图片）与多标签阅读（分栏、浮动、弹出窗口、进度还原）。
- 知识库绑定：右键绑定 bookcase；CBETA 三棵目录树只选一棵（默认部類），数据包里另加的知识库（`a/` 下的 HTML 文章）可同时绑定，各自建索引；搜索按知识库分组。
- 后续：标题快搜等，见规范第 11 节。

## 目录

```
CLAUDE.md             给编码代理的接手说明（先读）
docs/design-spec.md   设计规范与 CBETA 格式附录
docs/session-notes.md 设计会话纪要：决策、依据、验证结果
docs/reference/       旧项目 cbeta-explorer 的截图，bookcase.zip 目录截图
core/                 smart-library-core：解析与装配（Rust 库）
cli/                  smart-library-cli：命令行检查工具（可执行文件 smart-library）
app/                  桌面程序：Svelte 前端（src/）与 Tauri 命令层（src-tauri/）
samples/cbeta/        真实数据样本与黄金输出
data/bookinfo.json    人工整理的经籍元数据（典类、朝代、作译者），4826 条
data/t2s.txt          繁体到简体单字映射（来自 fast2s，MIT，见 t2s.LICENSE.md）
tools/                ref_extract.py（卷 XML 参考提取器）、diff_extract.py（与 Rust 逐卷对比）、convert_bookinfo.py
```

## 开始

```
cargo test -p smart-library-core
cargo run -p smart-library-cli --release -- check C:\Devx\smartLib.d\bookcase.zip
```

Windows 控制台先执行 `chcp 65001` 以正确显示中文。`bookcase.zip` 不在仓库中，请放在仓库之外。

## 运行桌面程序

需要 Node.js 和 WebView2（Windows 10/11 一般已装）。

```
cd app
npm install
npm run tauri dev
```

第一次打开时点左侧「＋ 绑定」（或在左侧空白处右键）绑定 bookcase.zip，再右键知识库「建立索引」。

Windows 自动更新与 GitHub Releases 发布步骤见 [发布说明](docs/windows-release.md)。
