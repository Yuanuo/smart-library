<script>
  // Export a library, or the picked nodes of it, as a package (ZIP with a
  // manifest and one Markdown file per document): backup, move, sharing.
  import { onMount } from "svelte";
  import { save } from "@tauri-apps/plugin-dialog";
  import * as api from "./api.js";
  import DialogHeader from "./DialogHeader.svelte";

  // `sel`: { whole, keys, rels } as the export command takes them; `label`: what is exported
  // `name`: what the file is named after (the node, the parent of the picked nodes, or the library)
  let { lib, sel, label, name, nodePath = "", onclose, ondone } = $props();

  let format = $state("smartlib");
  let withFiles = $state(true);
  let busy = $state(false);
  let error = $state("");
  let progress = $state(null); // { done, total }
  let result = $state(null);

  onMount(() => {
    if (lib.kind === "folder") format = "smartdir";
    const un = api.onExportProgress((p) => p.lib === lib.id && (progress = p));
    return () => un.then((f) => f());
  });

  const safe = (s) => s.replace(/[\\/:*?"<>|]+/g, "_").trim() || "export";

  /** yyyyMMddHH: the version of the export */
  function stamp() {
    const d = new Date();
    const p = (n) => String(n).padStart(2, "0");
    return `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}${p(d.getHours())}`;
  }

  async function run() {
    if (busy) return;
    error = "";
    busy = true;
    const isDir = lib.kind === "folder" && format === "smartdir";
    const ext = isDir ? "smartdir" : "smartlib";
    let out;
    try {
      out = await save({
        title: "导出到",
        defaultPath: `${safe(name || lib.title)}-${stamp()}.${ext}`,
        filters: [
          isDir
            ? { name: "文件夹知识库数据包（*.smartdir）", extensions: ["smartdir"] }
            : { name: "知识库数据包（*.smartlib）", extensions: ["smartlib"] },
        ],
      });
    } catch (e) {
      error = String(e);
      busy = false;
      return;
    }
    if (!out) {
      busy = false;
      return;
    }
    progress = { done: 0, total: 1, stage: "正在准备导出…" };
    try {
      result = await api.exportLibrary({
        lib: lib.id,
        whole: sel.whole,
        keys: sel.keys ?? [],
        rels: sel.rels ?? [],
        with_files: withFiles,
        format: isDir ? "smartdir" : "smartlib",
        title: sel.whole ? lib.title : (name || label || lib.title),
        out,
      });
      ondone?.(result);
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && !busy && onclose()}>
  <div class="dialog dialog-shell" role="dialog" aria-modal="true" aria-label="导出" tabindex="-1"
    onkeydown={(e) => e.key === "Escape" && !busy && onclose()}>
    <DialogHeader title={`导出：${sel.whole ? lib.title : label}`} path={nodePath || (sel.whole ? lib.title : "")} />
    <div class="dialog-content">

    {#if lib.kind === "folder"}
      <div class="fmt-group" role="radiogroup" aria-label="导出格式">
        <label class="fmt-opt" class:active={format === "smartdir"} class:disabled={busy}>
          <input type="radio" name="export-fmt" value="smartdir" bind:group={format} disabled={busy} />
          <div>
            <div class="fmt-title">文件夹数据包（<code>.smartdir</code>）</div>
            <div class="muted small">原样保留目录结构、原始文件、<code>.meta.json</code> 与引用的图片附件（不含回收站）。导入时可还原为本地文件夹知识库，也可导入为内部知识库。</div>
          </div>
        </label>
        <label class="fmt-opt" class:active={format === "smartlib"} class:disabled={busy}>
          <input type="radio" name="export-fmt" value="smartlib" bind:group={format} disabled={busy} />
          <div>
            <div class="fmt-title">通用数据包（<code>.smartlib</code>）</div>
            <div class="muted small">转换为 Markdown 与 <code>manifest.json</code>。导入时成为内部知识库。</div>
          </div>
        </label>
      </div>
      {#if format === "smartlib"}
        <label class="check" class:disabled={busy}><input type="checkbox" bind:checked={withFiles} disabled={busy} /> 同时打包原文件（图片、PDF 等非文本文件）</label>
      {/if}
    {:else}
      <p class="muted small">
        数据包（<code>.smartlib</code>，内部是 ZIP）：<code>manifest.json</code>（目录树、标题、来源）加上每篇一个 Markdown 文件。
        {#if lib.kind === "cbeta" || lib.kind === "html"}CBETA 的编注、异读和梵汉对照转成脚注，导入后仍可悬停查看；图片不导出。{/if}
        导入时成为一个可编辑的自建知识库。
      </p>
    {/if}
    {#if busy || (progress && !result)}
      {#if progress?.stage}
        <progress></progress>
        <p class="muted small">{progress.stage}{progress.total > 1 && progress.done > 0 && progress.stage === "正在打包附件…" ? `（${progress.done} / ${progress.total}）` : ""}</p>
      {:else}
        <progress max={progress?.total || 1} value={progress?.done || 0}></progress>
        <p class="muted small">已处理 {progress?.done ?? 0} / {progress?.total ?? "?"} {format === "smartdir" ? "项" : "篇"}，请勿关闭窗口…</p>
      {/if}
    {/if}
    {#if result}
      <p class="ok">已导出 {result.docs} {format === "smartdir" ? "个文件" : "篇"}{result.files ? `、${result.files} 个${format === "smartdir" ? "附件" : "原文件"}` : ""}：<span class="path">{result.path}</span></p>
    {/if}
    {#if error}<p class="error">{error}</p>{/if}
    </div>
    <div class="actions">
      <button onclick={onclose} disabled={busy}>{result ? "关闭" : "取消"}</button>
      {#if !result}<button class="primary" onclick={run} disabled={busy}>{busy ? "正在导出…" : "选择位置并导出…"}</button>{/if}
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
  .fmt-group { display: flex; flex-direction: column; gap: 8px; margin: 6px 0 10px; }
  .fmt-opt {
    display: flex; gap: 8px; align-items: flex-start; padding: 8px 10px;
    border: 1px solid var(--border); border-radius: 6px; cursor: pointer;
  }
  .fmt-opt.active { border-color: var(--accent); background: var(--hover); }
  .fmt-opt.disabled, .check.disabled { opacity: 0.55; cursor: not-allowed; }
  .fmt-opt input { margin-top: 3px; }
  .fmt-title { font-weight: 500; margin-bottom: 2px; }
  .check { display: flex; gap: 6px; align-items: center; margin: 8px 0; }
  progress { width: 100%; }
  .path { overflow-wrap: anywhere; user-select: text; }
  .ok { color: var(--note); }
  .error { color: var(--hl); }
  .actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 14px; }
  button:disabled { opacity: 0.55; cursor: not-allowed; }
</style>
