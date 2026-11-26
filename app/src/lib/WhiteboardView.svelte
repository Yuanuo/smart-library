<script>
  // A whiteboard note: the scene of Excalidraw (JSON) is the note's content.
  // Excalidraw is a React component; it is mounted into a plain element here
  // and loaded when the first board is opened. It is saved a moment after every
  // change to the drawing (not for moving the view or selecting), and with Ctrl+S.
  import { onMount, onDestroy, untrack } from "svelte";
  import * as api from "./api.js";
  import TitleLink from "./TitleLink.svelte";
  import Icon from "./Icon.svelte";
  import { matchesShortcut, formatShortcut } from "./shortcuts.js";
  import { settings } from "./settings.svelte.js";

  let { vol, title = vol.title, lib, path, readonly = false, onsaved = null, onlocate = null, ontag = null, onprops = null, extra = null, tabMenuItems = null } = $props();

  let el = $state();
  let error = $state("");
  let status = $state("");
  let root = null;
  let boardApi = $state(null);
  let convertToExcalidrawElements = null;
  let timer = null;
  let dirty = false;
  let gone = false;
  let blocked = $state(false);
  let pending = null; // the latest [elements, appState, files]
  let serialize = null;
  let seen = ""; // what the drawing looked like when it was last saved or loaded
  const start = untrack(() => vol.raw);

  const darkMedia = window.matchMedia("(prefers-color-scheme: dark)");
  let systemDark = $state(darkMedia.matches);
  const onDarkMedia = (e) => (systemDark = e.matches);
  darkMedia.addEventListener("change", onDarkMedia);
  onDestroy(() => darkMedia.removeEventListener("change", onDarkMedia));

  const dark = $derived(settings.theme === "dark" || (settings.theme === "auto" && systemDark));
  const isDark = () => dark;

  $effect(() => {
    const nextTheme = dark ? "dark" : "light";
    if (boardApi) boardApi.updateScene({ appState: { theme: nextTheme } });
  });

  const signature = (elements, files) =>
    elements.map((e) => `${e.id}:${e.version}`).join(",") + "|" + Object.keys(files ?? {}).join(",");

  export const getRaw = () =>
    !blocked && pending && serialize ? serialize(pending[0], pending[1], pending[2], "local") : null;
  export const isDirty = () => !readonly && dirty;

  function changed(elements, appState, files) {
    if (readonly) return;
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
    if (readonly || blocked || !dirty || !pending || !serialize) return;
    dirty = false;
    const [elements, appState, files] = pending;
    try {
      const nextStamp = await api.saveNote(lib, path, serialize(elements, appState, files, "local"));
      if (nextStamp) onsaved?.(nextStamp);
      if (!gone) status = "已保存";
    } catch (e) {
      dirty = true;
      error = String(e);
    }
  }

  function key(e) {
    if (!readonly && matchesShortcut(e, "save")) {
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
        convertToExcalidrawElements = ex.convertToExcalidrawElements;
        let initial = null;
        if (start && start.trim()) {
          try {
            const d = JSON.parse(start);
            if (!d || typeof d !== "object" || !Array.isArray(d.elements)) {
              throw new Error("缺少 elements 数组");
            }
            const files = d.files ?? {};
            initial = { elements: d.elements, files, scrollToContent: true };
            seen = signature(initial.elements, files);
          } catch {
            blocked = true;
            error = "白板数据格式异常，已阻止空白覆盖（可通过工具栏「快照」恢复历史版本）";
            return;
          }
        } else {
          initial = { elements: [], files: {}, scrollToContent: true };
          seen = signature([], {});
        }
        root = createRoot(el);
        const h = React.createElement;
        root.render(
          h(
            "div",
            { style: { height: "100%", width: "100%", overflow: "clip" } },
            h(ex.Excalidraw, {
              excalidrawAPI: (api) => {
                boardApi = api;
                api?.updateScene({ appState: { theme: isDark() ? "dark" : "light" } });
              },
              initialData: initial,
              viewModeEnabled: readonly,
              onChange: readonly ? undefined : changed,
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
    if (!readonly && dirty) save();
    // let the last save go out before the component is taken down
    setTimeout(() => root?.unmount(), 0);
  });

  export function getText() {
    try {
      const els = boardApi ? boardApi.getSceneElements() : JSON.parse(start || "{}").elements || [];
      const texts = els
        .filter((e) => !e.isDeleted && e.text)
        .map((e) => `- ${e.text.replace(/\s+/g, " ").trim()}`);
      return `# ${title}\n${texts.join("\n")}`;
    } catch {
      return "";
    }
  }

  function parseWhiteboardSkeleton(input) {
    const trimmed = input.trim();
    const fenceMatch = trimmed.match(/```(?:excalidraw|json)?\s*([\s\S]*?)```/i);
    const body = fenceMatch ? fenceMatch[1].trim() : trimmed;
    const startIdx = body.indexOf("[");
    const endIdx = body.lastIndexOf("]");
    if (startIdx >= 0 && endIdx > startIdx) {
      try {
        const parsed = JSON.parse(body.slice(startIdx, endIdx + 1));
        if (Array.isArray(parsed) && parsed.length) return parsed;
      } catch {}
    }
    try {
      const obj = JSON.parse(body);
      if (Array.isArray(obj?.elements)) return obj.elements;
    } catch {}
    // Fallback: parse markdown headings / indented list into a hierarchical left-to-right diagram
    const parsedItems = [];
    let fallbackLine = null;
    let baseHeadingDepth = 0;
    for (const rawLine of body.split("\n")) {
      const trimmedEnd = rawLine.trimEnd();
      if (!trimmedEnd.trim()) continue;
      const trimmed = trimmedEnd.trimStart();
      if (trimmed.startsWith("```")) continue;
      const indent = rawLine.length - trimmed.length;
      const cleanLabel = (s) => s.replace(/\*\*(.+?)\*\*/g, "$1").trim();

      const hMatch = trimmed.match(/^(#+)\s+(.+)$/);
      if (hMatch) {
        const hashes = hMatch[1].length;
        baseHeadingDepth = hashes;
        parsedItems.push({ depth: hashes - 1, text: cleanLabel(hMatch[2]) });
        continue;
      }

      const bMatch = trimmed.match(/^(?:[-*+]|\d+[.、])\s+(.+)$/);
      if (bMatch) {
        const listDepth = Math.floor(indent / 2);
        const depth = baseHeadingDepth > 0 ? baseHeadingDepth + listDepth : listDepth;
        parsedItems.push({ depth, text: cleanLabel(bMatch[1]) });
      } else if (!parsedItems.length && !fallbackLine) {
        fallbackLine = cleanLabel(trimmed);
      }
    }
    if (!parsedItems.length && fallbackLine) {
      parsedItems.push({ depth: 0, text: fallbackLine });
    }
    if (!parsedItems.length) throw new Error("未识别到有效的白板图元或列表项");

    const minDepth = Math.min(...parsedItems.map((it) => it.depth));
    for (const it of parsedItems) it.depth -= minDepth;

    // Build tree forest
    const roots = [];
    const stack = [];
    for (const it of parsedItems) {
      const node = { text: it.text, depth: it.depth, children: [] };
      while (stack.length && stack[stack.length - 1].depth >= it.depth) {
        stack.pop();
      }
      if (stack.length) {
        stack[stack.length - 1].children.push(node);
      } else {
        roots.push(node);
      }
      stack.push(node);
    }

    // Measure & assign left-to-right tree coordinates
    const boxW = 230;
    const colGap = 72;
    const rowGap = 24;
    const boxHeight = (txt) => (txt.length > 26 ? 88 : 68);

    function measure(node) {
      node.h = boxHeight(node.text);
      if (!node.children.length) {
        node.spanH = node.h;
        return node.spanH;
      }
      let total = 0;
      node.children.forEach((c, idx) => {
        if (idx > 0) total += rowGap;
        total += measure(c);
      });
      node.spanH = Math.max(node.h, total);
      return node.spanH;
    }

    const out = [];
    function place(node, x, topY) {
      node.x = x;
      node.y = topY + (node.spanH - node.h) / 2;
      out.push({
        type: "rectangle",
        x: node.x,
        y: node.y,
        width: boxW,
        height: node.h,
        backgroundColor: node.depth === 0 ? "#f5e8dc" : "#faf6ee",
        strokeColor: "#8c3b2b",
        fillStyle: "solid",
        roundness: { type: 3 },
        label: { text: node.text.slice(0, 64), fontSize: 15 },
      });
      if (!node.children.length) return;
      const childrenTotal = node.children.reduce((acc, c, idx) => acc + c.spanH + (idx > 0 ? rowGap : 0), 0);
      let cy = topY + (node.spanH - childrenTotal) / 2;
      const childX = x + boxW + colGap;
      for (const child of node.children) {
        place(child, childX, cy);
        const sx = node.x + boxW + 4;
        const sy = node.y + node.h / 2;
        const ex = child.x - 4;
        const ey = child.y + child.h / 2;
        out.push({
          type: "arrow",
          x: sx,
          y: sy,
          width: ex - sx,
          height: ey - sy,
          points: [[0, 0], [ex - sx, ey - sy]],
          strokeColor: "#8c3b2b",
        });
        cy += child.spanH + rowGap;
      }
    }

    let curY = 80;
    for (const root of roots) {
      measure(root);
      place(root, 80, curY);
      curY += root.spanH + rowGap * 1.5;
    }
    return out;
  }

  export async function applyAi({ mode = "append", text }) {
    if (readonly || !boardApi) return;
    const rawItems = parseWhiteboardSkeleton(text);
    const hasFull = rawItems.every((e) => e.version != null && e.versionNonce != null);
    let newEls = !hasFull && convertToExcalidrawElements ? convertToExcalidrawElements(rawItems) : rawItems;
    const cur = boardApi.getSceneElements().filter((e) => !e.isDeleted);
    let finalEls;
    if (mode === "replace" || !cur.length) {
      finalEls = newEls;
    } else {
      let maxY = 0;
      for (const e of cur) {
        const b = (e.y || 0) + (e.height || 0);
        if (b > maxY) maxY = b;
      }
      let minY = Infinity;
      for (const e of newEls) {
        if ((e.y || 0) < minY) minY = e.y || 0;
      }
      const dy = (Number.isFinite(minY) ? maxY + 80 - minY : maxY + 80);
      finalEls = [...cur, ...newEls.map((e) => ({ ...e, y: (e.y || 0) + dy }))];
    }
    boardApi.updateScene({ elements: finalEls });
    boardApi.scrollToContent(newEls, { fitToContent: true });
    changed(finalEls, boardApi.getAppState(), boardApi.getFiles());
    await save();
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="board" onkeydown={key}>
  <div class="view-bar">
    <TitleLink {title} {onlocate} {tabMenuItems} />
    {#if ontag}<button type="button" class="tool-btn" onclick={ontag} title={formatShortcut("tag", settings.shortcuts) ? `为本篇加标签（${formatShortcut("tag", settings.shortcuts)}）` : "为本篇加标签"} aria-label="加标签"><Icon name="tag" size={16} /></button>{/if}
    {#if onprops}<button type="button" class="tool-btn" onclick={onprops} title={formatShortcut("props", settings.shortcuts) ? `属性（${formatShortcut("props", settings.shortcuts)}）` : "属性"} aria-label="属性"><Icon name="sliders" size={16} /></button>{/if}
    {@render extra?.()}
    {#if readonly}<span class="muted status">白板（只读）</span>{:else}<span class="muted status">{status}</span>{/if}
    {#if error}<span class="error">{error}</span>{/if}
    <span class="grow"></span>
  </div>
  <div class="canvas" bind:this={el}></div>
</div>

<style>
  .board { display: flex; flex-direction: column; height: 100%; min-height: 0; overflow: clip; }
  .error { color: var(--hl); font-size: 13px; }
  .canvas { flex: 1; min-height: 0; position: relative; overflow: clip; }
  /* Excalidraw's containers must not be scrolled by the browser (a focused popover does that) */
  .canvas :global(.excalidraw), .canvas :global(.excalidraw-container) { overflow: clip; }
</style>
