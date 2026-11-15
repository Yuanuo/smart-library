不要在没有证据（明确要求、真实报告、真实日志/事件，或可重现的真实故障）的情况下添加人为/推测的场景、限制、防护措施、路径、测试或标准。


## 前端弹窗规范（Dialog）
- 针对具体文档/节点的弹窗，统一使用 `.dialog.dialog-shell` 容器与 `DialogHeader` 组件（参考 `TagDialog.svelte`、`PropsDialog.svelte`）。
- 标题格式为 `<功能名>：<文档/节点名称>`（例如 `标签：${title}`、`文档快照：${title}`）。
- 标题下方通过 `DialogHeader` 的 `path` 属性展示文档全路径（优先使用 `libPanel.nodePath(...)` 获取完整树路径）。
- 弹窗主体使用 `.dialog-content`，底部按钮栏直接使用 `.actions`（遵循全局 `.dialog-shell > .actions` 样式，不要自定义 `.footer-actions`）。
