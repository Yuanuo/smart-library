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

- 已完成：M0a–M0c（读取、抽取、索引与搜索）、M1 界面、结构化阅读、多标签阅读、知识库绑定与 `a/` HTML 文章、知识库类型重构第 1–3 步（添加向导、外部文件夹 `folder.rs`、内部库 `notes.rs` + `markdown.rs`）、M2 标题快搜（`core/src/titles.rs`；`#` 命令与粘贴 CBETA 引用转到经文 `cite.rs`）；187 个测试通过。详见 session-notes 第 6 节。
- 待用户手动验证：拖动停靠、解除绑定、快搜弹窗、目录宽度拖动、在目录树中定位、切换标签保留位置、编注的六种显示方式与悬停浮层。
- 待用户手动验证（新）：添加向导与「＋」菜单、链接文件夹并建索引、文件夹树按需展开、在目录树中定位文件夹里的文件。
- 待用户手动验证（内部库）：新建内部库、新建/重命名/删除/上移下移笔记、编辑保存、建索引后搜索、删除知识库。
- 已完成（待点验）：目录树图标与分隔线、窗口标题、添加笔记的位置路径、最近打开（Ctrl+E，`history.db`）、从节点搜索标题和全文、设置（Ctrl+,）。
- 已完成（待点验，全文搜索重构）：搜索页是工作区标签，可开多个、可弹出新窗口；页内有搜索框、模糊/精确、多选过滤（部类/时域/人物/藏经/典类标签页，查找、数量/拼音排序、A–Z 跳转）和「范围」列表（目录树拖入或右键「加入搜索范围」）；空词可浏览全部；藏经分面改用藏经名（`INDEX_VERSION` 3，**要重建索引**）。详见 design-spec 8.4a。
- 已完成（待点验，节点属性）：节点右键「属性…」编辑部类/时域/人物/藏经/典类（和标签），存 `props.db`，替换数据默认值、文件夹/笔记下传，保存后增量更新索引，导出导入带 `props`。详见 design-spec 8.4a。
- 已完成（待点验，每日一句）：目录树下方喇叭按钮，定时（30 分钟检查，每天一句）随机抽句，角标 1，点开弹出；设置里可关闭、管理来源（目录树右键「加入每日一句来源」）；搜索页的「随机一句」已移除。
- 已完成（待点验，第九批）：新建入口改为菜单里直接列「新建笔记 / 脑图 / 白板」（自动取名「笔记 n」，不再有对话框，也没有「新建子笔记」）；日记（知识库「我的日记」，年/月/日，Ctrl+J、展开当月自动建今天、选中文字「发送到今日记」）；白板笔记（Excalidraw，React 挂载，字体本地提供）。装依赖要加 `--legacy-peer-deps`。
- 已完成（待点验，第八批）：笔记直接在排版编辑器里打开并自动保存（不再分阅读 / 编辑）；脑图的节点样式面板、插入图片、导出 HTML / XMind / 大纲。
- 已完成（待点验，第七批）：笔记里粘贴图片（附件，导出导入带图）；笔记编辑器换成 Milkdown 排版 + 源码两种方式。脑图类型的笔记（Mind-Elixir，`content_type = mindmap`，摘录可加入脑图，导出导入带数据）。
- 已完成（待点验，第六批）：摘录加入笔记（带来源链接，点链接回到原文位置）。目录树多选（Ctrl/Shift 点选，可作搜索范围、导出范围）。知识库导出 / 导入（ZIP：manifest.json + Markdown，CBETA 编注转脚注，导入成为自建知识库）。
- 已完成（待点验，第五批）：版本读法「無」、提示冒号对齐、HTML 文章「无|标注」按钮、复制原文件路径/目录树路径、标签提示的目录树路径、标题快搜分批加载与设置。
- 已完成（待点验，第四批）：切换标签时窗口标题、收藏窗口「只看当前文档」、编注提示按版本对齐、HTML 脚注「（注）」、复制路径。
- 已完成（待点验，第三批）：页内查找调用浏览器查找栏、选中文字时 Ctrl+G/H、窗口标题与 YY.MM.DD 版本号、去掉状态栏（目录树下方设置/消息/关于）、本机字体列表、点击目录节点同时展开、弹出窗口菜单与独立窗口面板（最近打开、收藏与标签）。
- 已完成（待点验）：阅读器选中文字菜单、打开过的节点标绿、书签/收藏/标签（Ctrl+B，`marks.db`）。
- 已完成（待点验，可编辑的文件夹库第一步）：外部文件夹可设为读写（添加向导勾选，或右键「改为读写模式…」，都要先确认规则）；`.md`、`.map.json`、`.wbd.json` 可编辑并自动保存，可新建 / 重命名 / 删除（删到 `.smart-library/trash`），保存前比对修改时间，文件名严格校验并显示「本地文件名限制」。已按反馈修正：改回只读后三种文件仍显示；新增「新建文件夹」；知识库图标按类型区分（数据包 / 本地只读 / 本地读写 / 内部库）；编辑后的索引改为全文搜索前成批补写。向导里「外部文件夹」已改称「本地文件夹」。见 design-spec 6.5、session-notes 5k。
- 已完成（待点验，可编辑的文件夹库第二步）：文件夹和文件可重命名、「移动到…」（属性、标记、最近打开的路径跟着改）；粘贴图片存 `.smart-library/attachments/`，Markdown 里是相对链接；属性、书签、收藏、标签写进 `.smart-library/meta.json` 随文件夹走（`core/src/sidecar.rs`）。111 个测试通过。第三步（待点验）：删除文件夹、多级新建文件夹、拖放移动（单个或多个）、图片集中放 `.smart-library/attachments/<前2位>/<3-4位>/`，每次粘贴是自己的文件（指纹 + 随机数，内容相同的用硬链接只占一份），不再引用时进回收站，设置里「清理缓存」清空；只读阅读器的 `<br>` 不再显示成文字）、窗口获得焦点时目录树与侧车文件和磁盘对账（展开状态保留）、锁定 / 解锁知识库菜单、导入后新笔记自动记入待索引。向导里选项改为内部知识库、本地文件夹、CBETA 数据包、ZIM。116 个测试通过。选中文字拖到可写节点上会用它新建笔记（拖放见 design-spec 6.5）。
- 已完成（待点验，旧版数据包）：导入没有 manifest.json 的旧版 `.smartlib`（html → 笔记，保留 base64 图片和标注；kmind → 脑图），见 design-spec 导出导入一节。
- 已完成（待点验，第十批；**`INDEX_VERSION` 4，要重建索引**）：内部库图片按引用计数、24 小时后清理；内部库笔记可拖到笔记或库根；时域表（设置里编辑 / 导入 / 导出，别称并入、年代排序，无 bookinfo 的经解析作译者）；异体字表 `data/variants.txt`；节点「别名」和快搜里的文件夹条目；本地文件夹读 PDF / docx / pptx / odt 文字；阅读器「简体」显示；当前文档快捷键（Ctrl+T 标签、D 书签、P 属性、L 定位、N 新建、W 关闭、Ctrl+Tab 切换、Ctrl+Shift+S 简体）。详见 design-spec。
- 已完成（待点验）：阅读器「底本读法」（正文换成大正藏读法）。
- 已完成（待点验）：阅读器「字重」三档（标准 / 稍粗 / 加粗）。正文与强调分用 `--reader-weight` / `--reader-strong-weight`，不用 `bolder`；稍粗、加粗两档在字体没有对应字面时靠描边（`--reader-stroke`）保证档位有效。见 design-spec 设置一节、session-notes 5o。
- 已完成（待点验）：标签右键菜单与标签栏下拉列表改成阅读器右键菜单的样式（`app.css`）；标签提示里的目录树路径去掉「目录：」前缀；目录树工具栏去掉「知识库」标题、图标按钮靠左，右端新增视图导航 ←/→（同 Ctrl+Shift+←/→，没有可导航记录时不可用）；右键箭头打开「视图历史」对话框（检索、直接跳转、清除记录，不做新窗口）。见 session-notes 5p。
- 已完成（待点验）：词典导入。设置 → 词典里可导入 TXT（`【词条】词义`，`\\n` 换行）、StarDict（`.ifo`/`.idx`/`.dict`，含 `.gz`/`.dict.dz`，文本、HTML、XDXF、图片类型）、MDict（`.mdx`/`.mdd`，引擎 1/2、zlib、`Encrypted="2"` 键块解密、`@@@LINK=` 重定向；LZO 和 `Encrypted="1"` 暂不支持）。转换成 `.words` 放在 `<数据目录>/dicts/` 并自动加入词典路径；词条自动补小写/简体/繁体变体，词义按指纹去重，MDD 图片进 `.words_extra`。生成器 `core/src/dict_build.rs`，导入器 `core/src/dict_import.rs`，命令行 `dict import/list/search/lookup`，样例和生成脚本在 `samples/dict/`、`tools/make_dict_samples.py`。见 design-spec「查词典与词典导入」。
- 已完成（待点验）：阅读器「版心宽度」（阅读器设置里 20–100 em，默认 46em，随字号缩放；可勾选「全宽」按可视区自适应，三种版式共用）；设置搜索页拼音匹配选项补上边距和分隔线；词典的「导入词典」改称「导入第三方词典」。
- 已完成（待点验）：阅读器版式改为「段落 | 折行 | 折页」，后两种是同一份结构化 HTML 用 CSS 折行（旧程序的做法，`Reader.svelte` 的 `.doc.wrap-lines` / `.doc.wrap-pages`：块级元素 `display: inline`，`.lb::before` 用 `content: "\A" attr(data-lb) " : "` 另起一行带出行号，`.pb::before` 在页首写出页号），所以编注、异文、缺字、底本读法、简体、命中高亮在两种折行下都可用；只有 HTML 里没有 `data-lb` 的卷（如 ZIM）仍退回按 `lines` 逐行渲染的纯文本。「行号」复选框常显，折行、折页时置灰，避免工具栏随版式重排。见 design-spec 阅读区版式一段。
- 已完成（待点验）：选字查词（设置 → 阅读器）：按住 Ctrl/Cmd 选字（默认）、选中即查、关闭；只在只读阅读器（`Reader.svelte`，含弹出的阅读窗口）里生效，笔记、脑图、白板不参与；选中超过 10 个字或带标点时不查；查词复用同一个词典窗口（`dict-select` + `dict-query`）。见 design-spec 设置一节。
- 已完成（待点验）：本地文件夹（读写）的回收站与恢复：删除时在 `.smart-library/trash.json` 记下原路径；目录树右键知识库 →「回收站…」可逐项或全部恢复，同名的文件夹会合并、同名的文件自动改名，笔记引用的图片一起回来；设置里的「清理缓存」仍会清空它。
- 已完成（待点验）：内部知识库的笔记回收站：删除时把整棵子树存成 `kind=deleted` 的快照（JSON 树，含原 id、上级、顺序）；知识库右键「回收站…」可逐项 / 全部恢复、永久删除、清空。恢复按删除时的**标题路径**重建：上级存在就重用，不存在就新建同名的空笔记；只有恢复的这一篇同名时才加数字，回收站里的其他条目不受影响（不会把上级的其他子笔记带回来）。每个节点原 id 空闲就沿用（日记固定 id 能回来），被占用才换新。快照还按旧 id 保存属性、别名和标签（`DeletedMeta`），恢复时按新旧 id 写回；恢复出的节点在目录树里默认折叠。删除期间用 `trash_refs` 留住引用的图片。见 design-spec 6.4。
- 已完成（待点验，第十一批）：文件夹数据包 `.smartdir`（本地文件夹库原样导出，含目录结构、原始文件、`.meta.json`、引用到的图片；导入时可选解压为空文件夹并绑定为本地库，或转为内部库；`ExportDialog` 选格式，`ImportDirDialog` 选方式）；目录树右键「在资源管理器中打开」；切回窗口时检测文件夹外部改动（索引目录的 `folder-snapshot.json`），提示新增 / 修改 / 删除篇数并自动重建索引；笔记编辑器排版跟随阅读器设置。187 个测试通过。见 design-spec 6.5 与「文件夹数据包」一节。
- 下一步待定：媒体标签、ZIM 适配器。

## 代码约定

- 版本号 `YY.MM.DD`：每次 `npm run tauri build` 前由 `tools/stamp-version.cjs` 按当天本地日期写入五处（`core`、`cli`、`app/src-tauri` 的 Cargo.toml、`tauri.conf.json`、`app/package.json`），并同步 `Cargo.lock`、`app/package-lock.json`；文件里是 semver（不补零），界面补零显示。GitHub Actions 直接跑 `tauri build`，不盖章，用标签的版本；发版当天先提交盖章后的文件，再打同日期的标签。

- 发版时更新 `CHANGELOG.md`：按版本号写一段（新增 / 改进 / 修复）。工作流会按版本号取这一节，转成纯文本（`### X` → `【X】`）填进草稿 Release 说明和 `latest.json` 的 `notes`；草稿里再改说明不会影响 `latest.json`，需要时替换该资产。

- `core` 不依赖界面或 Tauri；适配器只产生 `IndexDoc`，不碰 Tantivy。
- 渲染器分层：HTML 阅读器（`article.rs`）是通用实现，数据源差异写进 `Rules`（`PLAIN_RULES` / `ZIM_RULES`），不在通用分支里加 `if 源 == …`。CBETA 经文走自己的 `render.rs`。新数据源的兼容性改动只落在它自己的规则里，并配该源的测试，见 design-spec「渲染器分层」。
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
  #       dict import <文件或目录…> [--out DIR] / dict list / dict search 词 <词典…> / dict lookup 词 <词典…>

# 桌面程序（app/ 目录，第一次先 npm install）
npm run tauri dev        # 开发模式
npm run build            # 只构建前端，检查 Svelte 警告

# 抽取回归（--every 20 约 5 秒）
target\release\smart-library.exe volumes <bookcase.zip> XML/ --every 20 --out <临时目录>
python tools/diff_extract.py <bookcase.zip> <临时目录>      # 需要 lxml；不要加 python -I
```
