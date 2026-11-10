<script>
  // Export a library, or the picked nodes of it, as a package (ZIP with a
  // manifest and one Markdown file per document): backup, move, sharing.
  import { onMount } from "svelte";
  import { save } from "@tauri-apps/plugin-dialog";
  import * as api from "./api.js";

  // `sel`: { whole, keys, rels } as the export command takes them; `label`: what is exported
  // `name`: what the file is named after (the node, the parent of the picked nodes, or the library)
  let { lib, sel, label, name, onclose, ondone } = $props();

  let withFiles = $state(true);
  let busy = $state(false);
  let error = $state("");
  let progress = $state(null); // { done, total }
  let result = $state(null);

  onMount(() => {
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
    error = "";
    let out;
    try {
      out = await save({
        title: "导出到",
        defaultPath: `${safe(name || lib.title)}-${stamp()}.smartlib`,
        filters: [{ name: "知识库导出包（*.smartlib）", extensions: ["smartlib"] }],
      });
    } catch (e) {
      error = String(e);
      return;
    }
    if (!out) return;
    busy = true;
    progress = { done: 0, total: 1 };
    try {
      result = await api.exportLibrary({ lib: lib.id, whole: sel.whole, keys: sel.keys ?? [], rels: sel.rels ?? [], with_files: withFiles, out });
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
    <header class="dialog-header"><h2>导出</h2></header>
    <div class="dialog-content">
    <p><b>{lib.title}</b>：{sel.whole ? "整个知识库" : label}</p>
    <p class="muted small">
      导出包（<code>.smartlib</code>，内部是 ZIP）：<code>manifest.json</code>（目录树、标题、来源）加上每篇一个 Markdown 文件。
      {#if lib.kind === "cbeta" || lib.kind === "html"}CBETA 的编注、异读和梵汉对照转成脚注，导入后仍可悬停查看；图片不导出。{/if}
      导入时成为一个可编辑的自建知识库。
    </p>
    {#if lib.kind === "folder"}
      <label class="check"><input type="checkbox" bind:checked={withFiles} disabled={busy} /> 同时打包原文件（图片、PDF 等非文本文件）</label>
    {/if}
    {#if busy || (progress && !result)}
      <progress max={progress?.total || 1} value={progress?.done || 0}></progress>
      <p class="muted small">已处理 {progress?.done ?? 0} / {progress?.total ?? "?"} 篇</p>
    {/if}
    {#if result}
      <p class="ok">已导出 {result.docs} 篇{result.files ? `、${result.files} 个原文件` : ""}：<span class="path">{result.path}</span></p>
    {/if}
    {#if error}<p class="error">{error}</p>{/if}
    </div>
    <div class="actions">
      <button onclick={onclose} disabled={busy}>{result ? "关闭" : "取消"}</button>
      {#if !result}<button class="primary" onclick={run} disabled={busy}>选择位置并导出…</button>{/if}
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed; inset: 0; z-index: 950; background: rgba(0, 0, 0, 0.3);
    display: flex; align-items: center; justify-content: center;
  }
  .dialog { width: min(540px, calc(100vw - 32px)); }
  h2 { margin: 0 0 8px; font-size: 18px; color: var(--accent); }
  p { margin: 6px 0; }
  .small { font-size: 13px; }
  .check { display: flex; gap: 6px; align-items: center; margin: 8px 0; }
  progress { width: 100%; }
  .path { overflow-wrap: anywhere; user-select: text; }
  .ok { color: var(--note); }
  .error { color: var(--hl); }
  .actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 14px; }
</style>
