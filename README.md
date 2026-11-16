# Smart Library

Smart Library 是一个轻量、高速、本地优先（Local-First）的桌面知识库与阅读笔记软件，基于 **Tauri 2 + Rust + Tantivy + SQLite + Svelte 5** 构建，支持 Windows、macOS 与 Linux。

它最初面向 **CBETA《大藏经》**（`bookcase.zip`）的深度阅读与全文检索而设计，现已扩展为集**多源知识库管理、全文检索、结构化阅读、Markdown 笔记、思维导图、手绘白板、本地词典与 AI 助手**于一体的个人知识工作台。

---

## 核心特性

### 1. 多源知识库统一管理
- **内部知识库**：基于内置 SQLite 数据库存储，保存与检索速度极快，适合大量笔记与每日日记；支持导出/导入 `.zip` 备份包。
- **本地文件夹**：直接挂载磁盘目录（支持 `txt`、`md`、`html`、`pdf`、`docx` 等），可用 Git 或网盘自行同步；默认只读，可按需解锁为可控读写以编辑 Markdown、脑图与白板文件。
- **CBETA《大藏经》数据包**：支持 `bookcase.zip` 或解压目录，可绑定官方目录树（簡易 / 進階 / 部類）以及包内自建 HTML 知识库（`a/<code>/`）。
- **Wikipedia 离线包（ZIM）**：支持 `.zim` 离线百科文件的标题与全文索引、文章阅读及内部链接跳转。

### 2. 高速全文检索与快搜
- **Tantivy 本地检索引擎**：支持二字组、单字分词与繁简互查（基于 `data/t2s.txt`）。
- **多维筛选与定位**：支持按知识库分组、目录范围限定、分面筛选（典类、朝代、作译者等元数据），支持松散匹配与精确短语匹配，命中结果高亮并可直接跳转至卷内位置。
- **标题快搜与引用解析**：支持快速检索经名/文档标题，以及 CBETA 经典引文定位。

### 3. 结构化阅读与多窗口工作区
- **CBETA 经文排版**：支持段落、偈颂、科判、校注气泡与插图渲染。
- **阅读辅助**：支持繁简切换、行号与页码显示、划词标注与书签、文档历史快照、阅读历史与进度自动恢复。
- **多标签与自由布局**：基于 Dockview 的多标签与分栏工作区，支持将文档或面板弹出为独立窗口。

### 4. 笔记与知识创作
- **Markdown 编辑器**：集成 Milkdown，支持所见即所得与源码编辑。
- **思维导图**：集成 Mind Elixir，支持导出为 HTML 与 XMind 格式。
- **手绘白板**：集成 Excalidraw，支持自由绘图与灵感整理。
- **日常整理**：内置每日日记、剪藏、拼音注音、属性与标签管理。

### 5. 本地词典与 AI 助手
- **本地词典**：支持 `.words`（及 `.words_extra`）词典文件，支持阅读中划词/右键查词并在独立词典窗口查看释义。
- **AI 助手**：支持侧边面板与独立对话窗口，可结合当前阅读文档上下文进行问答、关联引用、辅助命名，并将对话结果保存为笔记。

---

## 安装与使用

### 下载安装包
前往 GitHub Releases 页面下载对应平台的安装包：
- **Windows**：NSIS 安装程序（`.exe`，支持应用内自动更新）
- **macOS**：Universal `.dmg` / `.app`
- **Linux**：`.deb` / `.rpm` / `.AppImage`

### 快速上手
1. 首次启动后，点击左侧边栏「**＋ 绑定**」（或在左侧空白处右键），选择要创建或绑定的知识库类型（内部知识库、本地文件夹、CBETA 数据包或 ZIM 离线包）。
2. 绑定外部数据源后，右键该知识库并选择「**建立索引**」，完成后即可在全局搜索框中进行全文检索。

---

## 本地开发与构建

### 环境要求
- **Rust**：Stable 工具链（Workspace 包含 `core`、`cli`、`app/src-tauri`）
- **Node.js**：建议 22 或以上版本
- **系统依赖**：
  - **Windows**：WebView2（Windows 10/11 通常已预装）
  - **Linux (Ubuntu/Debian)**：
    ```bash
    sudo apt-get update
    sudo apt-get install -y libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev patchelf
    ```

### 启动桌面端开发模式
```bash
cd app
npm install --legacy-peer-deps
npm run tauri dev
```

### 构建桌面安装包
```bash
cd app
npm run tauri build
```

### 运行测试与代码检查
```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

---

## 命令行工具（`smart-library-cli`）

项目提供命令行工具用于数据校验、文本抽取、离线建索引与检索测试（Windows 控制台建议先执行 `chcp 65001` 以正常显示中文）：

```bash
# 检查 CBETA 数据包完整性
cargo run -p smart-library-cli --release -- check <path/to/bookcase.zip>

# 查看数据包信息与目录树
cargo run -p smart-library-cli --release -- info <path/to/bookcase.zip>

# 建立索引并搜索
cargo run -p smart-library-cli --release -- index <path/to/bookcase.zip>
cargo run -p smart-library-cli --release -- search <path/to/bookcase.zip> <关键词>

# ZIM 离线包检查、建索引与搜索
cargo run -p smart-library-cli --release -- zim-info <file.zim>
cargo run -p smart-library-cli --release -- zim-index <file.zim> <index_dir>
cargo run -p smart-library-cli --release -- zim-search <file.zim> <index_dir> <关键词>
```

---

## 项目结构

```text
core/                 smart-library-core：解析、索引、搜索、笔记与格式转换（Rust 库）
cli/                  smart-library-cli：命令行工具（可执行文件 smart-library）
app/                  桌面客户端：Svelte 5 前端（src/）与 Tauri 2 后端（src-tauri/）
data/                 内置数据：bookinfo.json（经籍元数据）、t2s.txt（繁简映射，MIT）
docs/                 设计规范、会话纪要与发布说明
samples/              真实数据样本与测试输出
tools/                辅助脚本（参考提取器、格式转换、发布配置等）
```

---

## 相关文档与协议

- [设计规范](docs/design-spec.md)
- [自动更新与发布说明](docs/windows-release.md)
- `data/t2s.txt` 繁简映射表来自 fast2s（MIT 协议，见 `data/t2s.LICENSE.md`）
- 本项目基于 [MIT License](LICENSE) 开源
