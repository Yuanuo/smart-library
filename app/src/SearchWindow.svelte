<script>
  // A search page popped out into its own window. It keeps its own state;
  // opening a hit is handed to the main window (a volume is open in one place
  // only), and "dock" brings the whole search back as a tab there.
  import { emit } from "@tauri-apps/api/event";
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import SearchPage from "./lib/SearchPage.svelte";

  let { info } = $props();
  const win = getCurrentWebviewWindow();
  // `info` comes from the window URL and never changes
  const start = (() => info)();

  let s = $state({
    query: start.query || "",
    phrase: !!start.phrase,
    lib: start.library ?? null,
    libraryPinned: !!start.libraryPinned,
    filters: start.filters || {},
    scopes: start.scopes || [],
    tab: start.tab || "/bulei",
    page: start.page || 1,
    ran: !!start.ran,
    autorun: false,
    rerun: 0,
    focus: 0,
  });

  async function dock() {
    await emit("search-dock", {
      query: s.query, phrase: s.phrase, library: s.lib, libraryPinned: s.libraryPinned, filters: $state.snapshot(s.filters),
      scopes: $state.snapshot(s.scopes), tab: s.tab, page: s.page, ran: s.ran,
    });
    await win.close();
  }
</script>

<div class="win">
  <div class="bar">
    <span class="title">全文搜索</span>
    <button onclick={dock} title="把这个搜索放回主窗口的标签页">停靠回主窗口</button>
  </div>
  <div class="body">
    <SearchPage {s} onopen={(req) => emit("popout-open", req)}
      onsetup={(action) => emit("search-setup", action)}
      ontitle={(t) => win.setTitle(`${t} — Smart Library`).catch(() => {})} />
  </div>
</div>

<style>
  .win { display: flex; flex-direction: column; height: 100%; }
  .bar {
    display: flex; justify-content: space-between; align-items: center; gap: 12px;
    padding: 6px 12px; background: var(--panel); border-bottom: 1px solid var(--line); font-size: 14px;
  }
  .title { font-weight: 600; color: var(--accent); }
  .body { flex: 1; min-height: 0; }
</style>
