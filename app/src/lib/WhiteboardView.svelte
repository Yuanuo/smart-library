<script>
  // A whiteboard note: the scene of Excalidraw (JSON) is the note's content.
  // Excalidraw is a React component; it is mounted into a plain element here
  // and loaded when the first board is opened. It is saved a moment after every
  // change to the drawing (not for moving the view or selecting), and with Ctrl+S.
  import { onMount, onDestroy, untrack } from "svelte";
  import * as api from "./api.js";

  let { vol, title = vol.title, lib, path } = $props();

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

  // Excalidraw places its right-click menu by its own arithmetic (it mixes
  // window and board coordinates, and a dock panel has its own offsets), so
  // it can jump away from the pointer. Put it where a menu belongs: at the
  // pointer, above it (or left of it) when there is no room below (right of
  // it), and inside the board.
  let watch = null;
  let queued = false;
  let pointer = null; // where the last right-click was
  function clampMenus() {
    queued = false;
    if (!el) return;
    const box = el.getBoundingClientRect();
    for (const menu of el.querySelectorAll(".context-menu")) {
      // move the popover around the menu, not only the menu: its box takes the
      // clicks, and a box left behind makes a right-click there open the
      // browser's own menu instead of Excalidraw's
      const m = menu.closest(".popover") ?? menu;
      m.style.transform = "";
      const r = m.getBoundingClientRect();
      const at = pointer ?? { x: r.left, y: r.top };
      let left = at.x + r.width <= box.right ? at.x : at.x - r.width;
      let top = at.y + r.height <= box.bottom ? at.y : at.y - r.height;
      left = Math.max(box.left, Math.min(left, box.right - r.width));
      top = Math.max(box.top, Math.min(top, box.bottom - r.height));
      const dx = Math.round(left - r.left);
      const dy = Math.round(top - r.top);
      if (dx || dy) m.style.transform = `translate(${dx}px, ${dy}px)`;
    }
    watch?.takeRecords(); // what was changed here is not news
  }

  // a menu that reaches past the board makes the clipping panels around it
  // scroll (overflow: hidden can still be scrolled by the browser); put them back
  function unscroll(e) {
    const t = e.target;
    if (t === document || t === document.documentElement || t === document.body) {
      const r = document.scrollingElement;
      if (r && (r.scrollTop || r.scrollLeft)) r.scrollTo(0, 0);
      if (document.body.scrollTop || document.body.scrollLeft) document.body.scrollTo(0, 0);
    } else if (t instanceof Element && (t.contains(el) || (el.contains(t) && !t.closest(".popover, .context-menu")))) {
      // an element around the board, or one of Excalidraw's own containers (its popover takes the focus
      // and the browser scrolls the container to show it): the board moves by itself, never by scrolling
      if (t.scrollTop) t.scrollTop = 0;
      if (t.scrollLeft) t.scrollLeft = 0;
    }
  }

  onMount(() => {
    window.addEventListener("scroll", unscroll, true);
    el.addEventListener("contextmenu", (e) => (pointer = { x: e.clientX, y: e.clientY }), true);
    watch = new MutationObserver((list) => {
      if (queued) return;
      queued = true;
      requestAnimationFrame(clampMenus);
    });
    watch.observe(el, { childList: true, subtree: true, attributes: true, attributeFilter: ["style"] });
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
            { style: { height: "100%", width: "100%", overflow: "clip" } },
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
    watch?.disconnect();
    window.removeEventListener("scroll", unscroll, true);
    if (dirty) save();
    // let the last save go out before the component is taken down
    setTimeout(() => root?.unmount(), 0);
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="board" onkeydown={key}>
  <div class="bar">
    <strong>{title}</strong>
    <span class="muted status">{status}</span>
    {#if error}<span class="error">{error}</span>{/if}
  </div>
  <div class="canvas" bind:this={el}></div>
</div>

<style>
  .board { display: flex; flex-direction: column; height: 100%; min-height: 0; overflow: clip; }
  .bar { display: flex; align-items: center; gap: 10px; padding: 4px 12px; border-bottom: 1px solid var(--line); }
  .status { font-size: 12px; }
  .error { color: var(--hl); font-size: 13px; }
  .canvas { flex: 1; min-height: 0; position: relative; overflow: clip; }
  /* Excalidraw's containers must not be scrolled by the browser (a focused popover does that) */
  .canvas :global(.excalidraw), .canvas :global(.excalidraw-container) { overflow: clip; }
</style>
