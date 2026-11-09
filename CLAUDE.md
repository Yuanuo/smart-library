# CLAUDE.md

给接手开发的编码代理。先读本文件；要细节读 `docs/design-spec.md`（文档以它为准，改行为时同步更新），要决策依据读 `docs/session-notes.md`。

## 项目

**Smart Library**（简写 `smart-library`）：本地知识库桌面软件，核心是《大藏经》等大量文本的快速、精确搜索。Tauri 2 + Svelte 5 + Rust + Tantivy，仅桌面端。

- **知识库** = 目录树的根节点（没有唯一总根），知识库也只出现在根节点；绑定、索引、搜索范围都以它为单位（规范第 2 节）。
- `core/` 业务逻辑；`cli/` 命令行 `smart-library`；`app/` 桌面程序（`app/src-tauri` 只做参数转换）；`tools/` Python 参照抽取与对比。

## 与用户协作

- 用中文回复。用户熟悉 Java，Rust 是新手，解释 Rust 概念时可对照 Java。
- Windows；控制台输出中文前先 `chcp 65001`。
- 测试数据 `C:\Devx\smartLib.d\bookcase.zip`（CBETA 2025.Q1，另加 `zhibei` 索达吉堪布全集、`longzang` 乾隆大藏经两棵树，内容是 `a/` 下的 HTML）。
- 节省用量：界面改动只跑编译和测试，由用户自己点验；用户明确要求时才用截图自动操作。
- 旧项目 cbeta-explorer（Java、Solr、JavaFX）可作行为参照，源码不在本仓库。

## 当前状态

- 已完成：M0a–M0c（读取、抽取、索引与搜索）、M1 界面、结构化阅读、多标签阅读、知识库绑定与 `a/` HTML 文章、知识库类型重构第 1–3 步（添加向导、外部文件夹 `folder.rs`、内部库 `notes.rs` + `markdown.rs`）、M2 标题快搜（`core/src/titles.rs`；`#` 命令与粘贴 CBETA 引用转到经文 `cite.rs`）；98 个测试通过。详见 session-notes 第 6 节。
- 待用户手动验证：拖动停靠、解除绑定、快搜弹窗、目录宽度拖动、在目录树中定位、切换标签保留位置、编注的六种显示方式与悬停浮层。
- 待用户手动验证（新）：添加向导与「＋」菜单、链接文件夹并建索引、文件夹树按需展开、在目录树中定位文件夹里的文件。
- 待用户手动验证（内部库）：新建内部库、新建/重命名/删除/上移下移笔记、编辑保存、建索引后搜索、删除知识库。
- 已完成（待点验）：目录树图标与分隔线、窗口标题、添加笔记的位置路径、最近打开（Ctrl+E，`history.db`）、从节点搜索标题和全文、设置（Ctrl+,）。
- 已完成（待点验，全文搜索重构）：搜索页是工作区标签，可开多个、可弹出新窗口；页内有搜索框、模糊/精确、多选过滤（部类/时域/人物/藏经/典类标签页，查找、数量/拼音排序、A–Z 跳转）和「范围」列表（目录树拖入或右键「加入搜索范围」）；空词可浏览全部；藏经分面改用藏经名（`INDEX_VERSION` 3，**要重建索引**）。详见 design-spec 8.4a。
- 已完成（待点验，第九批）：新建入口改为菜单里直接列「新建笔记 / 脑图 / 白板」（自动取名「笔记 n」，不再有对话框，也没有「新建子笔记」）；日记（知识库「我的日记」，年/月/日，Ctrl+J、展开当月自动建今天、选中文字「发送到今日记」）；白板笔记（Excalidraw，React 挂载，字体本地提供）。装依赖要加 `--legacy-peer-deps`。
- 已完成（待点验，第八批）：笔记直接在排版编辑器里打开并自动保存（不再分阅读 / 编辑）；脑图的节点样式面板、插入图片、导出 HTML / XMind / 大纲。
- 已完成（待点验，第七批）：笔记里粘贴图片（附件，导出导入带图）；笔记编辑器换成 Milkdown 排版 + 源码两种方式。脑图类型的笔记（Mind-Elixir，`content_type = mindmap`，摘录可加入脑图，导出导入带数据）。
- 已完成（待点验，第六批）：摘录加入笔记（带来源链接，点链接回到原文位置）。目录树多选（Ctrl/Shift 点选，可作搜索范围、导出范围）。知识库导出 / 导入（ZIP：manifest.json + Markdown，CBETA 编注转脚注，导入成为自建知识库）。
- 已完成（待点验，第五批）：版本读法「無」、提示冒号对齐、HTML 文章「无|标注」按钮、复制原文件路径/目录树路径、标签提示的目录树路径、标题快搜分批加载与设置。
- 已完成（待点验，第四批）：切换标签时窗口标题、收藏窗口「只看当前文档」、编注提示按版本对齐、HTML 脚注「（注）」、复制路径。
- 已完成（待点验，第三批）：页内查找调用浏览器查找栏、选中文字时 Ctrl+G/H、窗口标题与 YY.MM.DD 版本号、去掉状态栏（目录树下方设置/消息/关于）、本机字体列表、点击目录节点同时展开、弹出窗口菜单与独立窗口面板（最近打开、收藏与标签）。
- 已完成（待点验）：阅读器选中文字菜单、打开过的节点标绿、书签/收藏/标签（Ctrl+B，`marks.db`）。
- 下一步待用户确定：简体显示、本地目录或笔记仓库、异体字表、快搜里的文件夹条目与别名。

## 代码约定

- 版本号 `YY.MM.DD`；文件里写 semver `26.11.8`（不补零），界面补零显示。发版改五处（见 design-spec）。

- `core` 不依赖界面或 Tauri；适配器只产生 `IndexDoc`，不碰 Tantivy。
- 永不修改源数据；索引等衍生数据都能删除后重建。
- 先让命令行跑通，再接界面。
- 新数据规则：先在 `samples/` 加样本，再写测试，再改代码，最后更新 design-spec。
- 分词器或索引字段一改，`search::INDEX_VERSION` 加一。
- 改抽取规则：`tools/ref_extract.py` 和 `core/src/volume.rs` 两边一起改，跑下面的抽取回归，必须全部一致，锚点一致率不能下降。
- 不提交 `bookcase.zip` 和索引目录。
- 已决定：LC、Y 两套未登记藏经不收录；菜单里取不到内容的条目保留，解析时忽略。

## 工具链注意

- 用 `stable-x86_64-pc-windows-gnu`；它的 gcc 编不过 lzma/zstd，避免带 C 代码的依赖：zip 用 `default-features = false, features = ["deflate"]`，tantivy 用 `default-features = false, features = ["mmap", "lz4-compression"]`。
- quick-xml 0.42：名称和文本是 `&str`，实体引用是 `Event::GeneralRef`。
- Vite 热更新偶尔不生效（「函数未定义」）：`touch` 该文件再按 F5；改 `workspace.svelte.js` 后整页刷新。
- 未验证：`npm run tauri build`；`nav::clean_nav_title` 的括号切分、中文数字转换是否与旧实现一致。

## 常用命令

```
cargo test -p smart-library-core
cargo run -p smart-library-cli --release -- <命令> <bookcase.zip> ...
  # 命令：info / tree <树> --depth 2 / find 法華 / book T0262 / check / text T0262 1
  #       volumes [XML/ --render] / libraries / index [--tree longzang --index <目录>]
  #       titles 法华 序品 [--tree bulei|longzang --limit 20]；也可以是 #T0001_001 等命令或 CBETA 引用文字
  #       search 如是我聞 [--phrase --scope bulei/阿含部類 --facet ... --page 2]

# 桌面程序（app/ 目录，第一次先 npm install）
npm run tauri dev        # 开发模式
npm run build            # 只构建前端，检查 Svelte 警告

# 抽取回归（--every 20 约 5 秒）
target\release\smart-library.exe volumes <bookcase.zip> XML/ --every 20 --out <临时目录>
python tools/diff_extract.py <bookcase.zip> <临时目录>      # 需要 lxml；不要加 python -I
```
