<script>
  // The original file of a document, read-only, in a window of its own so
  // that it can be put beside the reader and compared with what it shows.
  import { onMount } from "svelte";
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import { basicSetup } from "codemirror";
  import { EditorState } from "@codemirror/state";
  import { EditorView } from "@codemirror/view";
  import { xml } from "@codemirror/lang-xml";
  import { html } from "@codemirror/lang-html";
  import { markdown } from "@codemirror/lang-markdown";
  import { json } from "@codemirror/lang-json";
  import { oneDark } from "@codemirror/theme-one-dark";
  import * as api from "./lib/api.js";

  let { info } = $props();
  // `info` comes from the window URL and never changes
  const start = (() => info)();
  const win = getCurrentWebviewWindow();

  let file = $state("");
  let lang = $state("text");
  let text = $state("");
  let error = $state("");
  let loading = $state(true);
  let copied = $state(false);
  let host = $state();
  let view = null;

  async function load() {
    loading = true;
    error = "";
    try {
      const r = await api.rawSource(start.lib, start.path ?? null, start.bookId ?? null);
      file = r.file;
      lang = r.lang;
      text = r.text;
    } catch (e) {
      file = "";
      error = String(e);
    } finally {
      loading = false;
    }
  }

  const dark = () => {
    const t = document.documentElement.dataset.theme;
    return t === "dark" || (t !== "light" && matchMedia("(prefers-color-scheme: dark)").matches);
  };

  $effect(() => {
    // read these so that a reload (or the host becoming available) rebuilds
    text;
    lang;
    error;
    const el = host;
    view?.destroy();
    view = null;
    if (!el || error) return;
    const language = { xml, html, markdown, json }[lang];
    view = new EditorView({
      state: EditorState.create({
        doc: text,
        extensions: [
          basicSetup,
          ...(language ? [language()] : []),
          EditorState.readOnly.of(true),
          EditorView.editable.of(false),
          ...(dark() ? [oneDark] : []),
        ],
      }),
      parent: el,
    });
    return () => {
      view?.destroy();
      view = null;
    };
  });

  onMount(() => {
    win.setTitle(`${start.title || "原始数据"} — 原始数据 — Smart Library`).catch(() => {});
    load();
  });

  async function copyPath() {
    try {
      await navigator.clipboard.writeText(file);
      copied = true;
      setTimeout(() => (copied = false), 1200);
    } catch {}
  }
</script>

<div class="win">
  <div class="bar">
    <span class="file" title={file}>{file || (error ? "原始数据不可用" : "正在读取…")}</span>
    {#if copied}<span class="ok">已复制</span>{/if}
    <span class="grow"></span>
    <button onclick={copyPath} disabled={!file}>复制路径</button>
    <button onclick={load} disabled={loading}>刷新</button>
  </div>
  <div class="body">
    {#if error}
      <div class="msg">{error}</div>
    {:else if loading}
      <div class="msg">正在读取…</div>
    {:else}
      <div class="code" bind:this={host}></div>
    {/if}
  </div>
</div>

<style>
  .win { display: flex; flex-direction: column; height: 100%; }
  .bar {
    display: flex; align-items: center; gap: 10px; padding: 6px 12px;
    background: var(--panel); border-bottom: 1px solid var(--line); font-size: 13px;
  }
  .file {
    font-family: ui-monospace, Consolas, monospace; color: var(--muted);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .ok { color: var(--accent); white-space: nowrap; }
  .grow { flex: 1; }
  .body { flex: 1; min-height: 0; display: flex; }
  .msg { margin: auto; color: var(--muted); padding: 24px; text-align: center; }
  .code { flex: 1; min-width: 0; min-height: 0; overflow: hidden; }
  .code :global(.cm-editor) { height: 100%; font-size: 13px; }
  .code :global(.cm-scroller) { overflow: auto; }
</style>
