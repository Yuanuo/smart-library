# 更新日志

版本号是 `YY.MM.DD`，与发版日期一致。从 26.11.20 起在这里逐版记录；更早的版本请看 [GitHub Releases](https://github.com/Yuanuo/smart-library/releases)。

## 26.11.20 — 2026-11-20

### 新增

- **本地 Agent（MCP）服务**：程序运行期间在本机 `127.0.0.1`（默认端口 27123）提供 MCP 服务，Claude Code、Codex、Cursor、Cherry Studio、Antigravity 等客户端可以直接检索、阅读和整理知识库。设置 →「本地 Agent」里可开关、修改端口、设置访问令牌、选择是否允许写入，并勾选禁止访问的知识库；页面提供「复制给 Agent 自动配置」和 HTTP / stdio 两套配置 JSON。
- **`smart-library mcp-bridge`**：把 stdio 转成 HTTP 的桥接命令，供只支持 stdio 传输的 MCP 客户端使用。
- **AI 助手新工具**：标题快搜（`quick_search`）、词典查询（`dict_lookup`）、词条搜索（`dict_search`）；读取长文档支持 `offset` / `next_offset` 分段。
- **设置页新增「本地 Agent」标签页**，集中管理本地服务的开关、权限与客户端接入配置。

### 改进

- 绑定知识库、导入数据包后自动开始建立索引，不再需要右键手动建索引。
- AI 助手与本地 Agent 共用同一套工具：写操作前检查知识库是否可写，只读库直接拒绝；编辑前自动创建快照，删除进入回收站。
- 外部 Agent 写入后，界面自动刷新目录树和已打开的文档。
- 列表类工具只返回必要字段，全文搜索结果去掉分面等界面专用数据，交给大模型的内容更紧凑。

### 修复

- 使用 Anthropic 兼容网关（例如 DeepSeek 的 `/anthropic`）时无法获取模型列表：现在先请求网关路径，失败再回退到网关根地址的 `/v1/models`、`/models`。

### 文档

- README 写明 Windows 10 的系统基线，以及 WebView2 运行时的下载与排查方式。
