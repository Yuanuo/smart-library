<script>
  import DialogHeader from "./DialogHeader.svelte";

  let { lib, onconfirm, onclose } = $props();
  let deleteLocal = $state(false);
  let busy = $state(false);
  let error = $state("");

  async function submit() {
    if (busy) return;
    busy = true;
    error = "";
    try {
      await onconfirm(lib, deleteLocal);
    } catch (e) {
      error = String(e);
      busy = false;
    }
  }
</script>

<div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && !busy && onclose()}>
  <div
    class="dialog dialog-shell"
    role="dialog"
    aria-modal="true"
    aria-label={`解除绑定：${lib.title}`}
    tabindex="-1"
    onkeydown={(e) => e.key === "Escape" && !busy && onclose()}
  >
    <DialogHeader title={`解除绑定：${lib.title}`} path={lib.source} />
    <div class="dialog-content">
      <p>解除绑定后，该远程知识库将从目录树移除，其搜索索引会被删除；你可以随时重新添加。</p>
      {#if lib.remote}
        <p class="muted small">远程仓库：<code>{lib.remote}</code></p>
      {/if}
      <label class="opt">
        <input type="checkbox" bind:checked={deleteLocal} disabled={busy} />
        <span>同时删除本地数据文件夹</span>
      </label>
      {#if deleteLocal}
        <div class="danger-box" role="alert">
          <strong>高危警示：删除本地数据不可恢复！</strong>
          <p>
            将永久删除磁盘上的整个目录 <code>{lib.source}</code> 及其中的全部文件（包含你曾在该目录中手动做过的任何修改或未推送内容），删除后无法从回收站或本程序恢复。
          </p>
        </div>
      {/if}
      {#if error}<p class="error">{error}</p>{/if}
    </div>
    <div class="actions">
      <button onclick={onclose} disabled={busy}>取消</button>
      <button class={deleteLocal ? "danger" : "primary"} onclick={submit} disabled={busy}>
        {busy ? "处理中…" : deleteLocal ? "解除绑定并永久删除" : "解除绑定"}
      </button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed; inset: 0; z-index: 950; background: rgba(0, 0, 0, 0.35);
    display: flex; align-items: center; justify-content: center;
  }
  .dialog { width: min(540px, calc(100vw - 32px)); }
  .small { font-size: 13px; overflow-wrap: anywhere; }
  .opt { display: flex; gap: 8px; align-items: center; margin: 12px 0 8px; cursor: pointer; font-weight: 500; }
  .danger-box {
    margin: 10px 0;
    padding: 10px 12px;
    border: 1.5px solid var(--hl);
    border-radius: 6px;
    background: var(--hl-bg, rgba(200, 40, 40, 0.08));
    color: var(--hl);
    font-size: 13px;
    line-height: 1.55;
  }
  .danger-box p { margin: 4px 0 0; overflow-wrap: anywhere; }
  .error { color: var(--hl); font-size: 13px; }
  .actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 14px; }
  button.danger {
    background: var(--hl);
    color: var(--paper, #fff);
    border-color: var(--hl);
  }
  button:disabled { opacity: 0.55; cursor: not-allowed; }
</style>
