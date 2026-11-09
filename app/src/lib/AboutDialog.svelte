<script>
  // A short note about the program: version (YY.MM.DD) and where its data is.
  let { info, onclose } = $props();
</script>

<div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-label="关于" tabindex="-1"
    onkeydown={(e) => e.key === "Escape" && onclose()}>
    <h2>{info?.name ?? "Smart Library"}</h2>
    <p class="ver">版本 {info?.version ?? ""}（年.月.日）</p>
    <p>本地知识库桌面软件：快速、精确地阅读和检索《大藏经》等大量文本，也可以链接本机文件夹、写自己的笔记。</p>
    <p class="muted">数据位置：<span class="path">{info?.data_dir ?? ""}</span></p>
    <p class="muted keys">快捷键：双击 Shift / Ctrl+G 标题快搜　Ctrl+H 全文搜索　Ctrl+E 最近打开　Ctrl+B 收藏与标签　Ctrl+, 设置　Ctrl+J 今日日记</p>
    <p class="muted keys">当前文档：Ctrl+T 标签　Ctrl+D 书签　Ctrl+P 属性　Ctrl+L 在目录树中定位　Ctrl+N 新建笔记　Ctrl+W 关闭标签　Ctrl+Tab / Ctrl+Shift+Tab 切换标签　Ctrl+Shift+S 简体 / 原文　F2 重命名　Ctrl+S 保存</p>
    <div class="actions"><button class="primary" onclick={onclose}>关闭</button></div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed; inset: 0; z-index: 950; background: rgba(0, 0, 0, 0.3);
    display: flex; align-items: center; justify-content: center;
  }
  .dialog {
    width: min(480px, calc(100vw - 32px)); outline: none; background: var(--paper);
    border-radius: 10px; padding: 18px 24px; box-shadow: 0 10px 40px rgba(0, 0, 0, 0.25);
  }
  h2 { margin: 0 0 4px; font-size: 22px; color: var(--accent); }
  .ver { margin: 0 0 10px; color: var(--muted); }
  .path { overflow-wrap: anywhere; user-select: text; }
  .keys { font-size: 13px; }
  .actions { display: flex; justify-content: flex-end; margin-top: 8px; }
</style>
