<script>
  // Import a `.smartdir` folder package: either extract into an empty local directory
  // and bind it as a folder library, or import into a new internal notes library.
  import { onMount } from "svelte";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import * as api from "./api.js";
  import DialogHeader from "./DialogHeader.svelte";

  let { path, info, onclose, ondone } = $props();

  let mode = $state("folder"); // "folder" | "notes"
  let title = $state("");
  let dest = $state("");
  let writable = $state(false);
  let busy = $state(false);
  let error = $state("");

  onMount(() => {
    title = info?.title || "";
  });

  let picking = $state(false);
  const locked = $derived(busy || picking);

  async function pickDest() {
    if (locked) return;
    error = "";
    picking = true;
    try {
      const picked = await openDialog({
        directory: true,
        multiple: false,
        title: "选择用于解压的空文件夹",
      });
      if (picked) dest = picked;
    } catch (e) {
      error = String(e);
    } finally {
      picking = false;
    }
  }

  async function run() {
    if (locked) return;
    error = "";
    if (mode === "folder" && !dest.trim()) {
      error = "请先选择一个空的本地文件夹";
      return;
    }
    busy = true;
    try {
      const libTitle = title.trim() || null;
      const res =
        mode === "folder"
          ? await api.importDirAsFolder(path, dest, libTitle, writable)
          : await api.importPackage(path, null, null, libTitle);
      ondone?.(res, mode);
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && !locked && onclose()}>
  <div
    class="dialog dialog-shell"
    role="dialog"
    aria-modal="true"
    aria-label="导入文件夹数据包"
    tabindex="-1"
    onkeydown={(e) => e.key === "Escape" && !locked && onclose()}
  >
    <DialogHeader title={`导入数据包：${info.title}`} {path} />
    <div class="dialog-content">
      <p class="muted small">
        文件夹数据包（<code>.smartdir</code>），包含 {info.files} 个文件。
      </p>

      <label class="field">
        <span>知识库名称</span>
        <input type="text" bind:value={title} placeholder={info.title} disabled={locked} />
      </label>

      <div class="mode-group" role="radiogroup" aria-label="导入方式">
        <label class="mode-opt" class:active={mode === "folder"} class:disabled={locked}>
          <input type="radio" name="import-mode" value="folder" bind:group={mode} disabled={locked} />
          <div>
            <div class="mode-title">导入为本地文件夹知识库</div>
            <div class="muted small">原样解压到选定的空文件夹，并绑定为本地文件夹知识库。如需合并到已有文件夹，请解压到空目录后手动合并。</div>
          </div>
        </label>
        <label class="mode-opt" class:active={mode === "notes"} class:disabled={locked}>
          <input type="radio" name="import-mode" value="notes" bind:group={mode} disabled={locked} />
          <div>
            <div class="mode-title">导入为内部知识库</div>
            <div class="muted small">转换为内部可编辑知识库（保留目录树、元信息、脑图、白板与图片附件）。</div>
          </div>
        </label>
      </div>

      {#if mode === "folder"}
        <div class="field">
          <span>目标空文件夹</span>
          <div class="row">
            <input type="text" bind:value={dest} placeholder="请选择一个空目录…" readonly disabled={locked} />
            <button type="button" onclick={pickDest} disabled={locked}>选择…</button>
          </div>
        </div>
        <label class="check" class:disabled={locked}>
          <input type="checkbox" bind:checked={writable} disabled={locked} />
          绑定后允许编辑（可读写模式）
        </label>
      {/if}

      {#if busy}
        <progress></progress>
        <p class="muted small">{mode === "folder" ? "正在解压并绑定本地文件夹，请稍候…" : "正在导入到内部知识库，请稍候…"}</p>
      {/if}

      {#if error}<p class="error">{error}</p>{/if}
    </div>
    <div class="actions">
      <button onclick={onclose} disabled={locked}>取消</button>
      <button class="primary" onclick={run} disabled={locked || (mode === "folder" && !dest)}>
        {busy ? "正在导入…" : "开始导入"}
      </button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed; inset: 0; z-index: 950; background: rgba(0, 0, 0, 0.3);
    display: flex; align-items: center; justify-content: center;
  }
  .dialog { width: min(540px, calc(100vw - 32px)); }
  p { margin: 6px 0; }
  .small { font-size: 13px; }
  .field { display: flex; flex-direction: column; gap: 4px; margin: 10px 0; font-size: 13px; }
  .field input[type="text"] { width: 100%; box-sizing: border-box; }
  .row { display: flex; gap: 6px; }
  .row input { flex: 1; min-width: 0; }
  .mode-group { display: flex; flex-direction: column; gap: 8px; margin: 10px 0; }
  .mode-opt {
    display: flex; gap: 8px; align-items: flex-start; padding: 8px 10px;
    border: 1px solid var(--border); border-radius: 6px; cursor: pointer;
  }
  .mode-opt.active { border-color: var(--accent); background: var(--hover); }
  .mode-opt.disabled, .check.disabled { opacity: 0.55; cursor: not-allowed; }
  .mode-opt input { margin-top: 3px; }
  .mode-title { font-weight: 500; margin-bottom: 2px; }
  .check { display: flex; gap: 6px; align-items: center; margin: 8px 0; font-size: 13px; }
  progress { width: 100%; margin-top: 6px; }
  .error { color: var(--hl); font-size: 13px; }
  .actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 14px; }
  button:disabled { opacity: 0.55; cursor: not-allowed; }
</style>
