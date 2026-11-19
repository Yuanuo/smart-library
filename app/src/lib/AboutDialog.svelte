<script>
  // A short note about the program: version (YY.MM.DD) and what it does,
  // in words a reader (not a developer) understands.
  let { info, onclose, oncheckupdate } = $props();
</script>

<div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog dialog-shell" role="dialog" aria-modal="true" aria-label="关于" tabindex="-1"
    onkeydown={(e) => e.key === "Escape" && onclose()}>
    <header class="dialog-header"><h2>{info?.name ?? "Smart Library"}</h2></header>
    <div class="dialog-content">
      <p class="lead">一个放在你自己电脑上的书库：读书、查词、找出处，都在一个窗口里。轻便极速。</p>
      <ul class="points">
        <li><b>收集</b>：加入离线文库，也可以链接电脑上的文件夹，或者写自己的笔记。</li>
        <li><b>查找</b>：输入一句话、一个词或编号，立刻看到它出现在哪一部、哪一卷。</li>
        <li><b>阅读</b>：接近纸书的排版，注释、词典、跨书链接都在手边。</li>
        <li><b>保管</b>：内容存放在你自己的电脑上，随时可以备份或删除。</li>
      </ul>
    </div>
    <div class="actions">
      <span class="ver">版本 {info?.version ?? ""}</span>
      <button onclick={oncheckupdate}>检查更新</button><button class="primary" onclick={onclose}>关闭</button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed; inset: 0; z-index: 950; background: rgba(0, 0, 0, 0.3);
    display: flex; align-items: center; justify-content: center;
  }
  .dialog { width: min(600px, calc(100vw - 32px)); }
  h2 { margin: 0 0 4px; font-size: 22px; color: var(--accent); }
  .lead { margin: 0 0 14px; font-size: 15px; line-height: 1.7; }
  .points { margin: 0; padding: 0; list-style: none; display: grid; gap: 8px; }
  .points li { line-height: 1.7; padding-left: 2px; }
  .points b { color: var(--accent); font-weight: 600; margin-right: 2px; }
  .actions { display: flex; align-items: center; justify-content: flex-end; gap: 8px; }
  .ver { color: var(--muted); white-space: nowrap; }
</style>
