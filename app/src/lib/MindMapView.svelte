<script>
  // A mind map note: the data (Mind-Elixir JSON) is the note's content. It is
  // edited in place and saved a moment after every change (and with Ctrl+S).
  // A node's link to a place in a document (`smartlib://`) leads there.
  import { onMount, onDestroy, untrack } from "svelte";
  import { save as saveDialog } from "@tauri-apps/plugin-dialog";
  import * as api from "./api.js";
  import { parseSource } from "./clip.js";
  import ContextMenu from "./ContextMenu.svelte";
  import TitleLink from "./TitleLink.svelte";
  import Icon from "./Icon.svelte";
  import { matchesShortcut } from "./shortcuts.js";
  import { settings } from "./settings.svelte.js";

  let { vol, title = vol.title, lib, path, onjump = null, onexternal = null, onlocate = null, ontag = null, onprops = null, onai = null, extra = null } = $props();

  let el = $state();
  let mind = null;
  let mindReady = $state(false);
  let MindElixirClass = null;
  let timer = null;
  let dirty = false;
  let gone = false;
  let status = $state("");
  let error = $state("");
  let blocked = $state(false);
  let menu = $state(null);
  let wait = null; // watching for the board to get a size
  let pan = null; // a drag of the board with the middle button: where it was last
  const start = untrack(() => vol.raw);

  const dataText = () =>
    JSON.stringify({ ...mind.getData(), direction: mind.direction }, (k, v) => (k === "parent" ? undefined : v));
  export const getRaw = () => (mind && !blocked ? dataText() : null);

  function changed() {
    dirty = true;
    status = "未保存";
    clearTimeout(timer);
    timer = setTimeout(save, 800);
  }

  async function save() {
    if (blocked || !mind || !dirty) return;
    dirty = false;
    try {
      await api.saveNote(lib, path, dataText());
      if (!gone) status = "已保存";
    } catch (e) {
      dirty = true;
      error = String(e);
    }
  }

  const darkMedia = window.matchMedia("(prefers-color-scheme: dark)");
  let systemDark = $state(darkMedia.matches);
  const onDarkMedia = (e) => (systemDark = e.matches);
  darkMedia.addEventListener("change", onDarkMedia);
  onDestroy(() => darkMedia.removeEventListener("change", onDarkMedia));

  const dark = $derived(settings.theme === "dark" || (settings.theme === "auto" && systemDark));
  const isDark = () => dark;

  $effect(() => {
    const isD = dark;
    if (mindReady && mind && MindElixirClass) {
      mind.changeTheme(isD ? MindElixirClass.DARK_THEME : MindElixirClass.THEME);
    }
  });

  // ---- pictures: kept like the pictures of notes, a node refers to them as `attach:<name>`
  async function setPicture(file) {
    const node = mind?.currentNode;
    if (!node) {
      error = "请先选中一个节点，再插入图片。";
      return;
    }
    try {
      const name = await api.noteAttach(lib, await file.arrayBuffer(), file.type, path);
      const bmp = await createImageBitmap(file);
      const width = Math.min(bmp.width, 240);
      const height = Math.max(1, Math.round((bmp.height * width) / bmp.width));
      await mind.reshapeNode(node, { image: { url: `attach:${name}`, width, height } });
      error = "";
      changed();
    } catch (e) {
      error = String(e);
    }
  }

  function pickPicture() {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = "image/*";
    input.onchange = () => input.files?.[0] && setPicture(input.files[0]);
    input.click();
  }

  const pictureOf = (e) => [...(e.clipboardData?.files ?? [])].find((f) => f.type.startsWith("image/"));

  // ---- export: HTML, XMind, a Markdown outline (the file is written by the program)
  const safe = (s) => s.replace(/[\\/:*?"<>|]+/g, "_").trim() || "脑图";
  function stamp() {
    const d = new Date();
    const p = (n) => String(n).padStart(2, "0");
    return `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}${p(d.getHours())}`;
  }

  const MIME = { png: "image/png", jpg: "image/jpeg", gif: "image/gif", webp: "image/webp", bmp: "image/bmp" };

  /** The data with the pictures inside (a file for somewhere else cannot reach `attach:`). */
  async function withPictures(data) {
    const walk = async (n) => {
      if (n.image?.url?.startsWith("attach:")) {
        const name = n.image.url.slice(7);
        const buf = await api.notePicture(lib, name);
        const blob = new Blob([buf], { type: MIME[name.split(".").pop()] ?? "image/png" });
        n.image.url = await new Promise((ok) => {
          const r = new FileReader();
          r.onload = () => ok(r.result);
          r.readAsDataURL(blob);
        });
      }
      for (const c of n.children ?? []) await walk(c);
    };
    await walk(data.nodeData);
    return data;
  }

  const outline = (n, depth = 0) =>
    `${"  ".repeat(depth)}- ${(n.topic ?? "").replace(/\s+/g, " ")}\n` + (n.children ?? []).map((c) => outline(c, depth + 1)).join("");

  export function getText() {
    if (!mind) return "";
    const data = JSON.parse(dataText());
    const sel = mind.currentNode?.nodeObj?.topic;
    const base = outline(data.nodeData);
    return sel ? `当前选中节点：${sel}\n\n脑图大纲：\n${base}` : base;
  }

  export async function applyAi({ mode = "replace", text }) {
    if (!mind) return;
    const rawJson = await api.aiMindmapParse(text, title);
    const parsed = JSON.parse(rawJson);
    if (!parsed?.nodeData) throw new Error("未能解析出有效的脑图节点");
    if (mode === "replace") {
      mind.refresh(parsed);
    } else {
      const full = JSON.parse(dataText());
      const targetId = mind.currentNode?.nodeObj?.id;
      const findNode = (n, id) => {
        if (n.id === id) return n;
        for (const c of n.children ?? []) {
          const found = findNode(c, id);
          if (found) return found;
        }
        return null;
      };
      const parent = (targetId && findNode(full.nodeData, targetId)) || full.nodeData;
      parent.children = parent.children || [];
      const incomingRoot = parsed.nodeData;
      const normTopic = (s) => (s || "").trim().replace(/^[「『"']|[」』"':：]+$/g, "");
      const toAdd =
        incomingRoot.children?.length &&
        (normTopic(incomingRoot.topic) === normTopic(parent.topic) || normTopic(incomingRoot.topic) === normTopic(title))
          ? incomingRoot.children
          : [incomingRoot];
      parent.children.push(...toAdd);
      mind.refresh(full);
    }
    changed();
    await save();
  }

  async function exportAs(kind) {
    try {
      const data = JSON.parse(dataText());
      const kinds = {
        html: ["html", "网页（HTML）"],
        xmind: ["xmind", "XMind"],
        md: ["md", "Markdown 大纲"],
      };
      const [ext, name] = kinds[kind];
      const path = await saveDialog({
        title: "导出脑图",
        defaultPath: `${safe(vol.title)}-${stamp()}.${ext}`,
        filters: [{ name, extensions: [ext] }],
      });
      if (!path) return;
      let blob;
      if (kind === "html") blob = await (await import("@mind-elixir/export-html")).data2Html(await withPictures(data));
      else if (kind === "xmind") blob = await (await import("@mind-elixir/export-xmind")).data2Xmind(data, mind.version);
      else blob = new Blob([outline(data.nodeData)], { type: "text/markdown" });
      await api.saveBytes(path, new Uint8Array(await blob.arrayBuffer()));
      status = `已导出：${path}`;
      error = "";
    } catch (e) {
      error = String(e);
    }
  }

  function exportMenu(e) {
    const r = e.currentTarget.getBoundingClientRect();
    menu = {
      x: r.left,
      y: r.bottom + 2,
      items: [
        { label: "导出为网页（HTML）…", action: () => exportAs("html") },
        { label: "导出为 XMind…", action: () => exportAs("xmind") },
        { label: "导出为 Markdown 大纲…", action: () => exportAs("md") },
      ],
    };
  }

  onMount(async () => {
    try {
      const [{ default: MindElixir }, { zh_CN }, { default: nodeMenu }, info] = await Promise.all([
        import("mind-elixir"),
        import("mind-elixir/i18n"),
        import("@mind-elixir/node-menu"),
        api.appInfo(),
        import("mind-elixir/style.css"),
        import("@mind-elixir/node-menu/dist/style.css"),
      ]);
      if (gone) return;
      // a tab restored after a restart is hidden (it has no size) until it is first shown, and a
      // map made then is laid out wrong: wait for the first time the board has a size
      if (!el.clientWidth || !el.clientHeight) {
        await new Promise((ok) => {
          const ro = new ResizeObserver(() => {
            if (el.clientWidth && el.clientHeight) (ro.disconnect(), ok());
          });
          ro.observe(el);
          wait = ro;
        });
        wait = null;
        if (gone) return;
      }
      let data = null;
      if (start && start.trim()) {
        try {
          data = JSON.parse(start);
        } catch {}
        if (!data?.nodeData) {
          blocked = true;
          error = "脑图数据格式异常，已阻止空白覆盖（可通过工具栏「快照」恢复历史版本）";
          return;
        }
      } else {
        data = MindElixir.new(vol.title);
      }
      mind = new MindElixir({
        el,
        direction: typeof data.direction === "number" ? data.direction : MindElixir.SIDE,
        toolBar: true,
        keypress: true,
        allowUndo: true,
        contextMenu: {
          locale: zh_CN,
          extend: [
            {
              name: "插入图片…",
              onclick: () => {
                const cover = el?.querySelector(".context-menu");
                if (cover instanceof HTMLElement) cover.hidden = true;
                pickPicture();
              },
            },
            ...(onai
              ? [
                  {
                    name: "AI 扩充此节点…",
                    onclick: () => {
                      const cover = el?.querySelector(".context-menu");
                      if (cover instanceof HTMLElement) cover.hidden = true;
                      const topic = mind?.currentNode?.nodeObj?.topic || title;
                      onai({
                        text: topic,
                        prompt: `请围绕节点「${topic}」扩展生成 4～6 个下级分支与子节点（仅输出纯 Markdown 缩进列表，直接从下级分支开始，不要包含开场白或总结）。`,
                      });
                    },
                  },
                ]
              : []),
          ],
        },
        theme: isDark() ? MindElixir.DARK_THEME : MindElixir.THEME,
        imageProxy: (url) => (url.startsWith("attach:") ? `${info.asset_prefix}${lib}/attachments/${url.slice(7)}` : url),
        // nothing copied inside the map: a picture on the clipboard goes to the selected node
        pasteHandler: (e) => {
          const f = pictureOf(e);
          if (f) {
            e.preventDefault();
            setPicture(f);
          }
        },
      });
      // the node menu (font, colors, tag, icon, link, memo) in Chinese
      mind.locale = "zh_CN";
      mind.install(nodeMenu);
      MindElixirClass = MindElixir;
      await mind.init(data);
      mindReady = true;
      requestAnimationFrame(() => {
        if (!gone && mind) {
          mind.linkDiv();
          mind.toCenter();
        }
      });
      setTimeout(() => {
        if (!gone && mind) {
          mind.linkDiv();
          mind.toCenter();
        }
      }, 150);
      const ro = new ResizeObserver(() => {
        if (!gone && mind && el?.clientWidth && el?.clientHeight) {
          mind.linkDiv();
        }
      });
      ro.observe(el);
      wait = ro;
      // the right button is for the menu, the middle one moves the board (as in the whiteboard)
      el.addEventListener("contextmenu", again, true);
      el.addEventListener("pointerdown", down, true);
      el.addEventListener("pointermove", drag);
      el.addEventListener("pointerup", drop);
      el.addEventListener("pointercancel", drop);
      mind.bus.addListener("operation", changed);
      mind.bus.addListener("changeDirection", changed);
    } catch (e) {
      error = String(e);
    }
  });

  // The map's own menu covers the whole board (it closes on a left click only), so a
  // right-click on another node reached the cover, not the node. Close the menu and
  // give the right-click to whatever is under it.
  function again(e) {
    const cover = e.target;
    if (!(cover instanceof Element) || !cover.classList.contains("context-menu") || cover.hidden) return;
    e.preventDefault();
    e.stopPropagation();
    cover.hidden = true;
    const under = document.elementFromPoint(e.clientX, e.clientY);
    under?.dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: e.clientX, clientY: e.clientY, button: 2, buttons: 2, view: window }),
    );
  }

  function down(e) {
    if (e.pointerType !== "mouse") return;
    if (e.button === 2) {
      e.stopImmediatePropagation(); // no dragging with it: only the menu
    } else if (e.button === 1) {
      e.preventDefault();
      e.stopImmediatePropagation();
      pan = { x: e.clientX, y: e.clientY };
      el.setPointerCapture?.(e.pointerId);
    }
  }
  function drag(e) {
    if (!pan || !mind) return;
    mind.move(e.clientX - pan.x, e.clientY - pan.y);
    pan = { x: e.clientX, y: e.clientY };
  }
  function drop(e) {
    if (!pan) return;
    pan = null;
    el.releasePointerCapture?.(e.pointerId);
  }

  onDestroy(() => {
    wait?.disconnect();
    gone = true;
    clearTimeout(timer);
    if (!blocked && mind && dirty) api.saveNote(lib, path, dataText()).catch(() => {});
    mind?.destroy?.();
  });

  /** A node's link: to a place in a document, or an external URL. */
  function click(e) {
    const a = e.target.closest?.("a");
    if (!a) return;
    e.preventDefault();
    e.stopPropagation();
    const raw = a.getAttribute("href") || "";
    const to = parseSource(raw);
    if (to && onjump) Promise.resolve(onjump(to)).catch((err) => (error = String(err)));
    else if (/^https?:\/\//i.test(raw)) onexternal?.(raw);
  }

  function key(e) {
    if (matchesShortcut(e, "save")) {
      e.preventDefault();
      clearTimeout(timer);
      save();
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="mm" onkeydown={key}>
  <div class="bar">
    <TitleLink {title} {onlocate} />
    {#if ontag}<button type="button" class="tool-btn" onclick={ontag} title="为本篇加标签（Ctrl+T）" aria-label="加标签"><Icon name="tag" size={16} /></button>{/if}
    {#if onprops}<button type="button" class="tool-btn" onclick={onprops} title="属性（Ctrl+P）" aria-label="属性"><Icon name="sliders" size={16} /></button>{/if}
    {@render extra?.()}
    <button type="button" class="tool-btn" onclick={exportMenu}>导出 ▾</button>
    <span class="muted hint" title="脑图 · 双击节点编辑，Tab 添加子节点，Enter 添加同级节点，右键更多，按住中键拖动画布">脑图 · 双击节点编辑，Tab 添加子节点，Enter 添加同级节点，右键更多，按住中键拖动画布</span>
    <span class="muted status">{status}</span>
  </div>
  {#if error}<p class="error">{error}</p>{/if}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="map" bind:this={el} onclickcapture={click}></div>
</div>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} items={menu.items} onclose={() => (menu = null)} />
{/if}

<style>
  .mm { display: flex; flex-direction: column; height: 100%; }
  .bar { display: flex; gap: 10px; align-items: center; padding: 6px 12px; border-bottom: 1px solid var(--line); font-size: 13px; min-width: 0; }
  .hint { flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .status { flex-shrink: 0; white-space: nowrap; }
  .tool-btn {
    display: inline-flex; align-items: center; gap: 4px; flex-shrink: 0; white-space: nowrap;
    padding: 3px 8px; line-height: 1; font-size: 13px;
    border: 1px solid var(--line); border-radius: 5px;
    background: var(--paper); color: var(--accent); cursor: pointer;
  }
  .tool-btn:hover { background: var(--accent-soft); border-color: var(--accent); }
  .map { flex: 1; min-height: 0; position: relative; }
  .error { color: var(--hl); padding: 4px 12px; margin: 0; }
</style>
