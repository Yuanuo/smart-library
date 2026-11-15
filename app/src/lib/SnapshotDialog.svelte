<script>
  // Modal dialog for managing per-document snapshots (manual + temporary AI snapshots).
  import { onMount } from "svelte";
  import * as api from "./api.js";
  import DialogHeader from "./DialogHeader.svelte";

  let {
    lib,
    docId,
    title = "",
    nodePath = "",
    getCurrentContent = null,
    onrestored = null,
    onclose,
  } = $props();

  let items = $state([]);
  let loading = $state(true);
  let busy = $state(false);
  let label = $state("");
  let error = $state("");
  let preview = $state(null); // { id, content, content_type }
  let restoredPrompt = $state(null); // snapshot item just restored, prompting whether to delete

  function formatTime(sec) {
    if (!sec) return "";
    const d = new Date(sec * 1000);
    const pad = (n) => String(n).padStart(2, "0");
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
  }

  function formatBytes(n) {
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
    return `${(n / (1024 * 1024)).toFixed(2)} MB`;
  }

  async function load() {
    loading = true;
    error = "";
    try {
      items = await api.listSnapshots(lib, docId);
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  onMount(load);

  async function createManual() {
    if (busy) return;
    busy = true;
    error = "";
    try {
      const content = getCurrentContent ? await getCurrentContent() : null;
      const lbl = label.trim() || "手动快照";
      await api.createSnapshot(lib, docId, "manual", lbl, content);
      label = "";
      await load();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function togglePreview(item) {
    if (preview?.id === item.id) {
      preview = null;
      return;
    }
    error = "";
    try {
      const rec = await api.getSnapshot(item.id);
      if (rec) preview = rec;
    } catch (e) {
      error = String(e);
    }
  }

  async function restore(item) {
    if (busy) return;
    busy = true;
    error = "";
    try {
      const rec = await api.restoreSnapshot(item.id);
      await onrestored?.(rec);
      restoredPrompt = item;
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function remove(item) {
    if (busy) return;
    busy = true;
    error = "";
    try {
      await api.deleteSnapshot(item.id);
      if (preview?.id === item.id) preview = null;
      if (restoredPrompt?.id === item.id) restoredPrompt = null;
      await load();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function onKeydown(e) {
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="backdrop" onclick={onclose} onkeydown={onKeydown}>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="dialog dialog-shell" role="dialog" aria-modal="true" aria-label={`文档快照：${title || "未命名"}`} tabindex="-1" onclick={(e) => e.stopPropagation()}>
    <DialogHeader title={`文档快照：${title || "未命名"}`} path={nodePath || docId} />

    <div class="dialog-content">
      <div class="create-bar">
        <input
          type="text"
          bind:value={label}
          placeholder="快照备注（可选，例如：改写前 / 初稿）"
          onkeydown={(e) => e.key === "Enter" && createManual()}
        />
        <button type="button" class="primary" onclick={createManual} disabled={busy}>
          创建快照
        </button>
      </div>

      {#if error}
        <p class="error">{error}</p>
      {/if}

      {#if restoredPrompt}
        <div class="prompt-bar">
          <span>已还原到「{restoredPrompt.label || "快照"}」（{formatTime(restoredPrompt.created_at)}），是否删除此快照？</span>
          <div class="prompt-actions">
            <button type="button" class="danger" onclick={() => remove(restoredPrompt)} disabled={busy}>删除此快照</button>
            <button type="button" onclick={() => (restoredPrompt = null)} disabled={busy}>保留</button>
          </div>
        </div>
      {/if}

      <div class="list">
        {#if loading}
          <p class="muted empty">载入快照中…</p>
        {:else if !items.length}
          <p class="muted empty">暂无快照。你可以点击上方「创建快照」手动保存当前版本；AI 修改文档前也会自动建立临时快照。</p>
        {:else}
          {#each items as item (item.id)}
            <div class="row" class:active={preview?.id === item.id}>
              <div class="meta">
                <div class="top">
                  <span class="badge" class:ai={item.kind === "ai"}>{item.kind === "ai" ? "AI 临时" : "手动"}</span>
                  <span class="lbl">{item.label || (item.kind === "ai" ? "AI 修改前" : "手动快照")}</span>
                </div>
                <div class="sub muted">
                  <span>{formatTime(item.created_at)}</span>
                  <span>·</span>
                  <span>{formatBytes(item.size_bytes)}</span>
                </div>
              </div>
              <div class="row-actions">
                <button type="button" onclick={() => togglePreview(item)} disabled={busy}>
                  {preview?.id === item.id ? "收起" : "预览"}
                </button>
                <button type="button" class="primary-outline" onclick={() => restore(item)} disabled={busy}>
                  还原
                </button>
                <button type="button" class="danger-outline" onclick={() => remove(item)} disabled={busy}>
                  删除
                </button>
              </div>
            </div>
            {#if preview?.id === item.id}
              <pre class="preview">{preview.content}</pre>
            {/if}
          {/each}
        {/if}
      </div>
    </div>

    <div class="actions">
      <button type="button" onclick={onclose}>关闭</button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.3);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1200;
  }
  .dialog {
    width: min(640px, calc(100vw - 32px));
  }
  .dialog-content {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .create-bar {
    display: flex;
    gap: 8px;
    padding: 0 0 12px;
    border-bottom: 1px solid var(--line);
  }
  .create-bar input {
    flex: 1;
    min-width: 0;
    padding: 6px 10px;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--bg);
    color: var(--ink);
    font-size: 13px;
  }
  .error {
    color: var(--hl);
    margin: 0;
    padding: 8px 0;
    font-size: 13px;
    border-bottom: 1px solid var(--line);
  }
  .prompt-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 10px 12px;
    margin-top: 8px;
    border-radius: 6px;
    background: var(--accent-soft);
    border: 1px solid var(--accent);
    font-size: 13px;
  }
  .prompt-actions {
    display: flex;
    gap: 6px;
    flex-shrink: 0;
  }
  .list {
    flex: 1;
    overflow: auto;
    padding: 4px 0 8px;
  }
  .empty {
    text-align: center;
    padding: 28px 12px;
    font-size: 13px;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 8px;
    border-bottom: 1px solid var(--line);
  }
  .row.active {
    background: var(--bg);
  }
  .meta {
    min-width: 0;
    flex: 1;
  }
  .top {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
  }
  .badge {
    padding: 1px 6px;
    border-radius: 4px;
    font-size: 11px;
    background: var(--accent-soft);
    color: var(--accent);
    flex-shrink: 0;
  }
  .badge.ai {
    background: rgba(180, 120, 40, 0.14);
    color: #9a5b13;
  }
  .lbl {
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sub {
    display: flex;
    gap: 6px;
    font-size: 12px;
    margin-top: 3px;
  }
  .row-actions {
    display: flex;
    gap: 6px;
    flex-shrink: 0;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  .row-actions button,
  .prompt-actions button {
    padding: 4px 10px;
    font-size: 12px;
  }
  button.primary-outline {
    color: var(--accent);
  }
  button.danger {
    background: var(--hl);
    color: #fff;
    border-color: var(--hl);
  }
  button.danger-outline {
    color: var(--hl);
  }
  .preview {
    margin: 0 0 8px;
    padding: 10px 12px;
    max-height: 220px;
    overflow: auto;
    background: var(--bg);
    border: 1px solid var(--line);
    border-radius: 6px;
    font-size: 12px;
    white-space: pre-wrap;
    word-break: break-word;
  }
</style>
