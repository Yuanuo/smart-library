<script>
  import { updateState as state, checkUpdates, downloadUpdate, installUpdate } from "./updater.svelte.js";
  const mb = (n) => (n / 1048576).toFixed(1);
</script>

{#if state.visible}
  <div class="backdrop" role="presentation">
    <div class="dialog dialog-shell" role="dialog" aria-modal="true" aria-label="软件更新" tabindex="-1"
      onkeydown={(e) => { if (e.key === "Escape" && !["downloading", "installing"].includes(state.phase)) state.visible = false; }}>
      <header class="dialog-header"><h2>软件更新</h2></header>
      <div class="dialog-content">
        {#if state.phase === "checking"}<p>正在检查更新…</p>
        {:else if state.phase === "current"}<p>当前已是最新版本。</p>
        {:else if state.phase === "error"}<p>无法检查更新，请检查网络和更新配置。</p><p class="error">{state.error}</p>
        {:else if state.phase === "available"}<p>发现新版本 <strong>{state.version}</strong>。</p>{#if state.notes}<div class="notes">{state.notes}</div>{/if}
        {:else if state.phase === "downloading"}<p>正在下载更新：{mb(state.downloaded)} MB{state.total ? ` / ${mb(state.total)} MB` : ""}</p>{#if state.total}<progress value={state.downloaded} max={state.total}></progress>{/if}
        {:else if state.phase === "ready"}<p>更新已下载并验证。安装时应用会关闭并重新启动，请先保存所有窗口中尚未保存的内容。</p>
        {:else if state.phase === "installing"}<p>正在启动安装程序…</p>
        {:else}<p>更新失败，请重试。</p><p class="error">{state.error}</p>{/if}
      </div>
      <div class="actions">
        {#if !["downloading", "installing"].includes(state.phase)}<button onclick={() => (state.visible = false)}>稍后</button>{/if}
        {#if state.phase === "available"}<button class="primary" onclick={downloadUpdate}>下载更新</button>
        {:else if state.phase === "ready"}<button class="primary" onclick={installUpdate}>安装并重启</button>
        {:else if ["error", "download-error", "install-error"].includes(state.phase)}<button class="primary" onclick={() => checkUpdates(true)}>重新检查</button>{/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop { position: fixed; inset: 0; z-index: 1100; background: rgba(0, 0, 0, .3); display: flex; align-items: center; justify-content: center; }
  .dialog { width: min(500px, calc(100vw - 32px)); }
  h2 { font-size: 18px; color: var(--accent); }
  p { margin: 0 0 8px; }
  .notes { white-space: pre-wrap; overflow-wrap: anywhere; font-size: 13px; max-height: 40vh; overflow-y: auto; }
  .error { color: var(--hl); overflow-wrap: anywhere; }
  .actions { display: flex; justify-content: flex-end; gap: 8px; }
  progress { width: 100%; }
</style>
