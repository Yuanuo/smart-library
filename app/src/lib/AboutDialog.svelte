<script>
  // A short note about the program: version (YY.MM.DD) and what it does.
  let { info, onclose, oncheckupdate } = $props();
</script>

<div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog dialog-shell" role="dialog" aria-modal="true" aria-label="关于" tabindex="-1"
    onkeydown={(e) => e.key === "Escape" && onclose()}>
    <header class="dialog-header"><h2>{info?.name ?? "Smart Library"}</h2></header>
    <div class="dialog-content">
    <p class="ver">版本 {info?.version ?? ""}</p>
    <p>本地知识库桌面软件：快速、精确地阅读和检索《大藏经》等大量文本，也可以链接本机文件夹、写自己的笔记。</p>
    </div>
    <div class="actions"><button onclick={oncheckupdate}>检查更新</button><button class="primary" onclick={onclose}>关闭</button></div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed; inset: 0; z-index: 950; background: rgba(0, 0, 0, 0.3);
    display: flex; align-items: center; justify-content: center;
  }
  .dialog { width: min(480px, calc(100vw - 32px)); }
  h2 { margin: 0 0 4px; font-size: 22px; color: var(--accent); }
  .ver { margin: 0 0 10px; color: var(--muted); }
  .actions { display: flex; gap: 8px; justify-content: flex-end; margin-top: 8px; }
</style>
