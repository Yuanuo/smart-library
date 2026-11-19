<script>
  // The note view: a visual (WYSIWYG) Markdown editor, Milkdown / Crepe. It is
  // the reader and the editor of an internal note at once. The note stays
  // Markdown; `onchange` gets the text after every edit (the owner saves it).
  // Pictures pasted, dropped or chosen are kept by `onpicture(file)` (-> name)
  // and written as `![](attach:<name>)`. `value` is read once: to change it
  // from outside, make the editor again.
  //
  // Like the reader it shows the hits of a search (`query`), goes to a line
  // (`target`, `¶n`: the n-th paragraph, heading or list item; `nonce` asks
  // again), reports the line at the top (`onprogress`), leads the source links
  // of clippings (`onjump`) and gives the selected text to a menu (`onmenu`).
  import { onMount, onDestroy, untrack } from "svelte";
  import { Crepe } from "@milkdown/crepe";
  import { insert } from "@milkdown/kit/utils";
  import { linkAttr } from "@milkdown/kit/preset/commonmark";
  import "@milkdown/crepe/theme/common/style.css";
  import "@milkdown/crepe/theme/frame.css";
  import * as api from "./api.js";
  import { parseSource } from "./clip.js";
  import { activeAiPreset, isAiReady, toBackendAiConfig } from "./settings.svelte.js";

  let {
    value,
    lib,
    onchange,
    onpicture,
    query = null,
    phrase = false,
    target = null,
    nonce = 0,
    onprogress = null,
    onjump = null,
    onexternal = null,
    onmenu = null,
  } = $props();

  let root = $state();
  let crepe = null;
  let gone = false;
  let ready = $state(false);
  let baseline = "";
  const initial = untrack(() => value);

  let prefix = "";
  async function assetPrefix() {
    if (!prefix) prefix = (await api.appInfo()).asset_prefix;
    return prefix;
  }

  /** `attach:<name>` is shown from the library's attachments. */
  async function proxy(url) {
    if (!url.startsWith("attach:")) return url;
    return `${await assetPrefix()}${lib}/attachments/${url.slice(7)}`;
  }

  const upload = async (file) => `attach:${await onpicture(file)}`;

  onMount(async () => {
    const F = Crepe.Feature;
    const aiEnabled = isAiReady();
    const aiProvider = (context, signal) => {
      const sys =
        "你是嵌入在 Markdown 笔记编辑器中的写作助手。\n\n规则：\n- 只输出 Markdown 正文，绝不要把输出包在 ```markdown 代码块里。\n- 不要输出开场白、解释或总结，只输出修改或生成后的正文内容。\n- 保留原有的 Markdown 结构（标题、列表、链接、代码块），除非指令要求改变。\n- 如果提供了 <selection>，只输出替换该选区的内容，不要重复上下文。\n- 如果没有 <selection>，输出在光标处顺畅续写的内容。";
      const parts = [`<document>\n${context.document}\n</document>`];
      if (context.selection) parts.push(`<selection>\n${context.selection}\n</selection>`);
      parts.push(`<instruction>\n${context.instruction}\n</instruction>`);
      return api.aiStreamGenerator(
        toBackendAiConfig(activeAiPreset()),
        sys,
        [{ role: "user", content: parts.join("\n\n") }],
        signal,
      );
    };
    const c = new Crepe({
      root,
      defaultValue: initial,
      features: { [F.AI]: aiEnabled },
      featureConfigs: {
        [F.ImageBlock]: { onUpload: upload, blockOnUpload: upload, inlineOnUpload: upload, proxyDomURL: proxy },
        [F.Placeholder]: { text: "写点什么……（输入 / 选择标题、列表、引文、表格、代码、图片；可以直接粘贴图片）" },
        ...(aiEnabled
          ? {
              [F.AI]: {
                provider: aiProvider,
                instructionPlaceholder: "输入 AI 改写或续写指令…",
                suggestionsHeaderLabel: "AI 建议",
                sendAsPromptHeaderLabel: "自定义指令",
                sendAsPromptLabel: "运行指令",
                submitButtonLabel: "发送",
                diff: { acceptLabel: "接受", rejectLabel: "拒绝" },
                streamingIndicator: { fallbackLabel: "AI 正在生成…", cancelHint: "按 Esc 取消" },
                diffActions: { retryLabel: "重试", rejectAllLabel: "拒绝修改", acceptAllLabel: "接受修改" },
              },
            }
          : {}),
      },
    });
    // Milkdown sanitizes custom schemes in href; retain our own safe link in
    // a data attribute while leaving its URL sanitizer in place.
    c.editor.config((ctx) => ctx.set(linkAttr.key, (mark) => {
      const href = mark.attrs.href || "";
      return parseSource(href) ? { "data-smartlib": href } : {};
    }));
    // the editor writes the text its own way when it loads: only a change after that counts
    c.on((l) =>
      l.markdownUpdated((_, md) => {
        if (gone) return;
        if (!ready) {
          baseline = md;
          return;
        }
        if (md !== baseline) {
          baseline = md;
          onchange(md);
        }
      }),
    );
    await c.create();
    if (gone) {
      c.destroy();
      return;
    }
    crepe = c;
    baseline = c.getMarkdown();
    setTimeout(() => {
      ready = true;
      afterLoad();
    }, 60);
  });

  onDestroy(() => {
    gone = true;
    CSS.highlights?.delete("sl-hit");
    crepe?.destroy();
  });

  // ---- lines, hits, progress
  const pm = () => root?.querySelector(".ProseMirror");
  /** The lines: headings, paragraphs and list items (a paragraph inside an item is the item). */
  const lines = () => [...(pm()?.querySelectorAll("h1, h2, h3, h4, h5, h6, p, li") ?? [])].filter((e) => !(e.tagName === "P" && e.closest("li")));

  function goToLine(lb) {
    const n = Number(String(lb ?? "").replace("¶", ""));
    if (!n) return false;
    const el = lines()[n - 1];
    if (!el) return false;
    el.scrollIntoView({ block: "start" });
    return true;
  }

  function showHits() {
    CSS.highlights?.delete("sl-hit");
    const q = (query ?? "").trim();
    const el = pm();
    if (!q || !el || !window.Highlight) return null;
    const terms = phrase ? [q] : q.split(/\s+/).filter(Boolean);
    const ranges = [];
    const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    for (let n = walker.nextNode(); n; n = walker.nextNode()) {
      const text = n.nodeValue;
      for (const t of terms) {
        for (let at = text.indexOf(t); at >= 0; at = text.indexOf(t, at + t.length)) {
          const r = new Range();
          r.setStart(n, at);
          r.setEnd(n, at + t.length);
          ranges.push(r);
        }
      }
    }
    if (ranges.length) CSS.highlights.set("sl-hit", new Highlight(...ranges));
    return ranges[0] ?? null;
  }

  function afterLoad() {
    const first = showHits();
    if (target && goToLine(target)) return;
    first?.startContainer.parentElement?.scrollIntoView({ block: "center" });
  }

  // a new search or jump from outside (the same note opened again)
  $effect(() => {
    const _t = [query, phrase, target, nonce];
    if (ready) untrack(afterLoad);
  });

  let progressTimer = null;
  function scrolled() {
    if (!onprogress) return;
    clearTimeout(progressTimer);
    progressTimer = setTimeout(() => {
      const top = root.getBoundingClientRect().top + 4;
      const all = lines();
      const i = all.findIndex((e) => e.getBoundingClientRect().bottom > top);
      if (i >= 0) onprogress(`¶${i + 1}`);
    }, 300);
  }

  /** The line of a point: the last one that starts before it. */
  function lineAt(node) {
    const el = node?.nodeType === 1 ? node : node?.parentElement;
    const own = el?.closest("h1, h2, h3, h4, h5, h6, p, li");
    const all = lines();
    const i = all.indexOf(own?.tagName === "P" ? (own.closest("li") ?? own) : own);
    return i >= 0 ? `¶${i + 1}` : null;
  }

  // ---- clicks and menus
  function click(e) {
    const a = e.target.closest?.("a");
    if (!a) return;
    const raw = a.dataset.smartlib || a.getAttribute("href") || "";
    const to = parseSource(raw);
    if (to) {
      e.preventDefault();
      e.stopPropagation();
      onjump?.(to);
      return;
    }
    if (/^https?:\/\//i.test(raw)) {
      e.preventDefault();
      e.stopPropagation();
      onexternal?.(raw);
    }
  }

  function menu(e) {
    const s = window.getSelection();
    const text = s && !s.isCollapsed && root.contains(s.anchorNode) ? s.toString().trim() : "";
    if (!text || !onmenu) return; // no selection: the browser's own menu (paste …)
    e.preventDefault();
    onmenu({ x: e.clientX, y: e.clientY, text, lb: lineAt(s.getRangeAt(0).startContainer) });
  }

  /** A picture on the clipboard: keep it and put it where the cursor is. */
  async function paste(e) {
    const files = [...(e.clipboardData?.files ?? [])].filter((f) => f.type.startsWith("image/"));
    if (!files.length || !crepe) return;
    e.preventDefault();
    e.stopPropagation();
    for (const f of files) {
      try {
        const name = await onpicture(f);
        crepe.editor.action(insert(`![](attach:${name})`));
      } catch (err) {
        console.warn("picture", err);
      }
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="note-editor" bind:this={root} onpastecapture={paste} onclickcapture={click} oncontextmenu={menu} onscroll={scrolled}></div>

<style>
  .note-editor { height: 100%; overflow: auto; background: var(--paper); }
  :global(::highlight(sl-hit)) { background: var(--hl-bg); color: var(--hl); }
  /* the editor takes the colors and the reading font of the program */
  .note-editor :global(.milkdown) {
    --crepe-color-background: var(--paper);
    --crepe-color-on-background: var(--ink);
    --crepe-color-surface: var(--panel);
    --crepe-color-surface-low: var(--bg);
    --crepe-color-on-surface: var(--ink);
    --crepe-color-on-surface-variant: var(--muted);
    --crepe-color-outline: var(--line);
    --crepe-color-primary: var(--accent);
    --crepe-color-secondary: var(--accent-soft);
    --crepe-color-on-secondary: var(--ink);
    --crepe-color-hover: var(--accent-soft);
    --crepe-color-selected: var(--accent-soft);
    --crepe-color-inline-area: var(--bg);
    --crepe-font-default: var(--reader-font, var(--font-serif));
    --crepe-font-title: var(--reader-font, var(--font-serif));
    min-height: 100%;
  }
  .note-editor :global(.milkdown .ProseMirror) { padding: 24px 56px 80px; font-size: var(--reader-size, 19px); line-height: var(--reader-line, 1.9); }
  .note-editor :global(a[data-smartlib]) { color: var(--accent); cursor: pointer; }
</style>
