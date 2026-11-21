<script>
  // A reading tab popped out into its own system window. It shows one
  // volume; progress and closing are reported to the main window, which owns
  // the saved workspace. Other volumes (juan / chapter in another volume)
  // open as tabs in the main window, so a volume is never open twice.
  import { onMount } from "svelte";
  import { emit } from "@tauri-apps/api/event";
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import ReaderPanel from "./lib/ReaderPanel.svelte";
  import PinyinDialog from "./lib/PinyinDialog.svelte";
  import { installShortcuts, readerSelection } from "./lib/shortcuts.js";

  let { info } = $props();
  // `info` comes from the window URL and never changes
  const start = (() => info)();
  const win = getCurrentWebviewWindow();

  let p = $state({
    lib: start.lib,
    bookId: start.bookId,
    path: start.path,
    query: null,
    phrase: false,
    target: start.lb,
    quiet: true,
    nonce: 0,
    reload: 0,
    editing: false,
    append: null,
    mode: start.mode || "para",
  });
  let title = $state(start.title || start.path);
  let tip = $state("");
  let docking = false;

  onMount(() => {
    const un1 = win.listen("popout-jump", (e) => {
      const q = e.payload || {};
      p.query = q.query || null;
      p.phrase = !!q.phrase;
      if (q.lb != null) {
        p.target = q.lb;
        p.quiet = false;
        p.nonce += 1;
      } else if (q.query) {
        p.target = null;
        p.quiet = false;
        p.nonce += 1;
      }
    });
    // closed by the user: forget this pop-out (not when the app exits)
    const un2 = win.onCloseRequested(() => {
      if (!docking) emit("popout-closed", { tab: start.tab });
    });
    // shortcuts are carried out by the main window
    const offKeys = installShortcuts((type) => {
      // about this window's document: its tags and bookmark; closing closes this window
      if (type === "tag" || type === "bookmark") return forward({ type, lb: lastLb, vol: { lib: start.lib, path: start.path, book_id: start.bookId, title, author: "" } });
      if (type === "close") return win.close();
      if (type === "props" || type === "locate" || type === "next-tab" || type === "prev-tab" || type === "nav-back" || type === "nav-forward") return;
      emit("popout-shortcut", { type, text: readerSelection().slice(0, 200) });
    });
    return () => {
      un1.then((f) => f());
      un2.then((f) => f());
      offKeys();
    };
  });

  let pinyinDlg = $state("");

  /** Menu actions (search, bookmarks, tags) are carried out by the main window. */
  function forward(a) {
    if (a.type === "pinyin" && a.text) {
      pinyinDlg = a.text;
      return;
    }
    const v = a.vol || {};
    emit("popout-action", {
      type: a.type, text: a.text, lb: a.lb, excerpt: a.excerpt,
      vol: { lib: v.lib, path: v.path, book_id: v.book_id, title: v.title, label: v.label, author: v.author },
    });
  }

  let lastLb = start.lb;
  async function dock() {
    docking = true;
    await emit("popout-dock", { tab: start.tab, lib: start.lib, path: start.path, bookId: start.bookId, lb: lastLb });
    await win.close();
  }
</script>

<div class="popout">
  <div class="bar">
    <span class="title" title={tip}>{title}</span>
    <button onclick={dock} title="把当前文档放回主窗口的标签页">停靠回主窗口</button>
  </div>
  <div class="body">
    <ReaderPanel
      {p}
      onopen={(req) => emit("popout-open", req)}
      onaction={forward}
      onprogress={(lb, mode) => {
        lastLb = lb;
        emit("popout-progress", { tab: start.tab, lb, mode });
      }}
      ontitle={(t, tooltip) => {
        title = t;
        tip = tooltip;
        win.setTitle(`${t} — Smart Library`).catch(() => {});
      }} />
  </div>
</div>

{#if pinyinDlg}
  <PinyinDialog text={pinyinDlg} onclose={() => (pinyinDlg = "")} />
{/if}

<style>
  .popout { display: flex; flex-direction: column; height: 100%; }
  .bar {
    display: flex; justify-content: space-between; align-items: center; gap: 12px;
    padding: 6px 12px; background: var(--panel); border-bottom: 1px solid var(--line); font-size: 14px;
  }
  .title { font-weight: 600; color: var(--accent); }
  .body { flex: 1; min-height: 0; }
</style>
