<script>
  // A whiteboard note: the scene of Excalidraw (JSON) is the note's content.
  // Excalidraw is a React component; it is mounted into a plain element here
  // and loaded when the first board is opened. It is saved a moment after every
  // change to the drawing (not for moving the view or selecting), and with Ctrl+S.
  import { onMount, onDestroy, untrack } from "svelte";
  import * as api from "./api.js";

  let { vol, lib, path } = $props();

  let el = $state();
  let error = $state("");
  let status = $state("");
  let root = null;
  let timer = null;
  let dirty = false;
  let gone = false;
  let pending = null; // the latest [elements, appState, files]
  let serialize = null;
  let seen = ""; // what the drawing looked like when it was last saved or loaded
  const start = untrack(() => vol.raw);

  const isDark = () => {
    const t = document.documentElement.getAttribute("data-theme");
    return t === "dark" || (!t && window.matchMedia("(prefers-color-scheme: dark)").matches);
  };

  const signature = (elements, files) =>
    elements.map((e) => `${e.id}:${e.version}`).join(",") + "|" + Object.keys(files ?? {}).join(",");

  function changed(elements, appState, files) {
    const sig = signature(elements, files);
    if (sig === seen) return; // only the view or the selection moved
    seen = sig;
    pending = [elements, appState, files];
    dirty = true;
    status = "未保存";
    clearTimeout(timer);
    timer = setTimeout(save, 1000);
  }

  async function save() {
    clearTimeout(timer);
    if (!dirty || !pending || !serialize) return;
    dirty = false;
    const [elements, appState, files] = pending;
    try {
      await api.saveNote(lib, path, serialize(elements, appState, files, "local"));
      if (!gone) status = "已保存";
    } catch (e) {
      dirty = true;
      error = String(e);
    }
  }

  function key(e) {
    if ((e.ctrlKey || e.metaKey) && e.key === "s") {
      e.preventDefault();
      save();
    }
  }

  onMount(() => {
    (async () => {
      try {
        window.EXCALIDRAW_ASSET_PATH = "/excalidraw-assets/";
        const [React, { createRoot }, ex] = await Promise.all([
          import("react"),
          import("react-dom/client"),
          import("@excalidraw/excalidraw"),
          import("@excalidraw/excalidraw/index.css"),
        ]);
        if (gone) return;
        serialize = ex.serializeAsJSON;
        let initial = null;
        try {
          const d = JSON.parse(start || "{}");
          const files = d.files ?? {};
          initial = { elements: d.elements ?? [], files, scrollToContent: true };
          seen = signature(initial.elements, files);
        } catch {}
        root = createRoot(el);
        const h = React.createElement;
        root.render(
          h(
            "div",
            { style: { height: "100%", width: "100%" } },
            h(ex.Excalidraw, {
              initialData: initial,
              onChange: changed,
              theme: isDark() ? "dark" : "light",
              langCode: "zh-CN",
              UIOptions: { canvasActions: { loadScene: false } },
            }),
          ),
        );
      } catch (e) {
        error = `白板组件载入失败：${e}`;
      }
    })();
  });

  onDestroy(() => {
    gone = true;
    if (dirty) save();
    // let the last save go out before the component is taken down
    setTimeout(() => root?.unmount(), 0);
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="board" onkeydown={key}>
  <div class="bar">
    <strong>{vol.title}</strong>
    <span class="muted status">{status}</span>
    {#if error}<span class="error">{error}</span>{/if}
  </div>
  <div class="canvas" bind:this={el}></div>
</div>

<style>
  .board { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  .bar { display: flex; align-items: center; gap: 10px; padding: 4px 12px; border-bottom: 1px solid var(--line); }
  .status { font-size: 12px; }
  .error { color: var(--hl); font-size: 13px; }
  .canvas { flex: 1; min-height: 0; position: relative; }
</style>
