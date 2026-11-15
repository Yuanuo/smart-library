<script module>
  // the note mode is shared by every reader of the window (and kept)
  const NOTE_MODES = [
    ["none", "无", "不显示编注，只读正文"],
    ["orig", "原编注", "在编注点直接嵌入原编注（大正藏等底本的校注）"],
    ["cb", "CB编注", "在编注点直接嵌入 CBETA 编注"],
    ["num", "数字标", "在编注点显示序号，鼠标移上去看 CBETA 编注"],
    ["color", "着色", "被改变的文字着色，鼠标移上去看 CBETA 编注和各版本读法"],
    ["color-orig", "着色+原编注", "被改变的文字着色，鼠标移上去看原编注、CBETA 编注和各版本读法"],
  ];
  const shared = $state({
    noteMode: (() => {
      try {
        const m = localStorage.getItem("reader.noteMode");
        return NOTE_MODES.some(([k]) => k === m) ? m : "color";
      } catch {
        return "color";
      }
    })(),
  });
</script>

<script>
  // One volume in two views:
  //  * 段落 (default): the structured HTML of the volume (design-spec A.6):
  //    divisions, headings, verses, lists, tables, 科判 diagrams, images,
  //    inline notes; collation notes and variant readings shown by the note
  //    mode (design-spec A.6); line numbers can be shown in the text;
  //  * 原书行: one original line per row, line number in the margin.
  // `target` is a line (lb) to scroll to (again whenever `nonce` changes);
  // `quiet` restores a position without the flash. While the reader scrolls,
  // `onprogress(lb)` reports the first line at the top of the view.
  import { tick } from "svelte";
  import Icon from "./Icon.svelte";
  import TitleLink from "./TitleLink.svelte";
  import ContextMenu from "./ContextMenu.svelte";
  import { makePinyinItem } from "./pinyinMenu.svelte.js";
  import { localDate } from "./diary.js";
  import * as api from "./api.js";
  import { getLast } from "./clipTarget.js";
  import { parseSource, sourceUrl } from "./clip.js";
  import { settings } from "./settings.svelte.js";
  import { ensureSimplifier, simplifyHtml, simplifyText } from "./simplify.svelte.js";

  let {
    vol,
    target = null,
    nonce = 0,
    quiet = false,
    mode = $bindable("para"),
    onvolume,
    onchapter,
    onprogress = null,
    onlocate = null,
    extra = null,
    // actions that need the main window (search, marks): `onaction({ type, text, lb, vol })`
    onaction = null,
    // open a place of a document (the source link of a clipping): `{ lib, bookId, path, lb, query, phrase }`
    onjump = null,
    onexternal = null,
    onai = null,
  } = $props();
  let scroller = $state();

  const load = (k, d) => {
    try {
      return localStorage.getItem(k) ?? d;
    } catch {
      return d;
    }
  };
  const save = (k, v) => {
    try {
      localStorage.setItem(k, v);
    } catch {}
  };
  let showLb = $state(load("reader.showLb", "0") === "1");
  $effect(() => save("reader.showLb", showLb ? "1" : "0"));
  $effect(() => save("reader.noteMode", shared.noteMode));
  const noteMode = $derived(shared.noteMode);
  // the base text's (大正藏) reading in place of CBETA's where they differ
  let baseText = $state(load("reader.baseText", "0") === "1");
  $effect(() => save("reader.baseText", baseText ? "1" : "0"));
  const simp = $derived(settings.readerScript === "simp");
  $effect(() => {
    if (simp) ensureSimplifier();
  });
  const html = $derived(simp ? simplifyHtml(vol?.html) : vol?.html);
  const hasFn = $derived(!!vol?.html && vol.html.includes('class="fn"')); // an article with footnotes
  const coloring = $derived(noteMode === "color" || noteMode === "color-orig");

  // note points without a changed reading (no `.app.v` with their id): in
  // the coloring modes they get a small mark, or they could not be seen
  $effect(() => {
    const _v = vol;
    const _m = mode;
    tick().then(() => {
      if (!scroller) return;
      const changed = new Set([...scroller.querySelectorAll(".app.v[data-n]")].map((a) => a.dataset.n));
      for (const n of scroller.querySelectorAll(".note")) {
        n.classList.toggle("lone", !n.dataset.n || !changed.has(n.dataset.n));
      }
    });
  });

  let restoring = false;
  $effect(() => {
    const t = target;
    const v = vol;
    const m = mode;
    const _n = nonce;
    const q = quiet;
    if (!v) return;
    restoring = true;
    tick().then(() => {
      if (!scroller) return;
      scroller.querySelectorAll(".flash").forEach((e) => e.classList.remove("flash"));
      const el = t ? scroller.querySelector(`[data-lb="${t}"]`) : null;
      if (el) {
        el.scrollIntoView({ block: q ? "start" : "center" });
        if (!q) el.classList.add("flash");
      } else {
        scroller.scrollTop = 0;
      }
      setTimeout(() => (restoring = false), 100);
    });
  });

  // reading progress: the first line marker at or below the top edge
  let timer = null;
  function onScroll() {
    if (!onprogress || restoring) return;
    clearTimeout(timer);
    timer = setTimeout(() => {
      if (!scroller) return;
      const top = scroller.getBoundingClientRect().top;
      for (const el of scroller.querySelectorAll("[data-lb]")) {
        if (el.getBoundingClientRect().top >= top - 2) {
          onprogress(el.dataset.lb);
          return;
        }
      }
    }, 400);
  }

  // hover tips instead of the browser's one-line title: notes and variant
  // readings (by the note mode), Sanskrit-Chinese pairs (`.tt`) and missing
  // glyphs (`.g`). A tip is groups of aligned rows [key, text], separated by
  // a line: the notes, then the reading in the text, then the other editions
  let tip = $state(null); // { groups: [[[key, text]]], el, x, top, bottom }
  let tipEl = $state();
  let tipLeft = $state(0);

  // the title moves to `data-tip`, so that the browser does not show it too
  function tipText(el) {
    if (el.hasAttribute("title")) {
      el.dataset.tip = el.getAttribute("title");
      el.removeAttribute("title");
    }
    return el.dataset.tip || "";
  }

  /** CBETA writes "nothing" as no text or `〔－〕` after the editions. */
  const reading = (t) => (!t.trim() || /^〔[－\-−]〕$/.test(t.trim()) ? "無" : t.trim());

  // the title at the left of the tool bar leaves out what the volume list next to it says
  // (the volume `卷一`; the whole title when the list shows the article's name)
  const shownTitle = $derived.by(() => {
    const now = vol?.volumes?.find((v) => v.path === vol.path)?.label ?? "";
    return now && now === vol.title ? "" : vol?.title ?? "";
  });

  function readings(text) {
    return text
      .split("；")
      .map((s) => s.trim())
      .filter(Boolean)
      .map((s) => {
        // several editions share a reading: `【宋】 【元】 【明】憋`
        const m = s.match(/^((?:\s*【[^】]*】)+)\s*(.*)$/);
        return m ? [m[1].replace(/\s+/g, ""), reading(m[2])] : ["", s];
      });
  }

  /** A CBETA note that only lists readings, `西天譯經【大】，宋西【明】`, as
   * aligned rows like the other readings: `【大】 西天譯經`; null for other notes. */
  function editionRows(text) {
    const segs = text.split(/[，；;]/).map((s) => s.trim()).filter(Boolean);
    if (!segs.length) return null;
    const rows = [];
    for (const s of segs) {
      const m = s.match(/^(.*?)((?:\s*【[^】]*】)+)$/);
      if (!m) return null;
      rows.push([m[2].replace(/\s+/g, ""), reading(m[1])]); // editions only: "nothing"
    }
    return rows;
  }

  function noteRows(note) {
    if (!note) return [];
    const orig = note.querySelector(".nb-orig")?.textContent;
    const cb = note.querySelector(".nb-cb")?.textContent;
    const rows = [];
    if (noteMode === "color-orig" && orig) rows.push(["原编注", orig]);
    if (cb || orig) {
      const text = cb || orig;
      const eds = editionRows(text);
      if (eds) rows.push(...eds);
      else rows.push([noteMode === "num" ? "" : "CBETA", text]);
    }
    return noteMode === "num" ? rows.filter((r) => r[0] !== "原编注") : rows;
  }

  function tipFor(el) {
    if (el.classList.contains("fn")) return [[["", el.dataset.fn || ""]]]; // footnote of an HTML article
    if (el.classList.contains("note")) return [noteRows(el)];
    if (el.classList.contains("app")) {
      const n = el.dataset.n;
      const note = n ? scroller.querySelector(`.note[data-n="${CSS.escape(n)}"]`) : null;
      const rs = readings(tipText(el));
      // the note often lists the same editions as the reading itself (`【大】如，宋元明【無】`):
      // then only what it adds (the original note, other text) comes first
      const listed = rs.some(([k]) => k.startsWith("【"));
      const rows = noteRows(note).filter((r) => !(listed && r[0].startsWith("【")));
      return [rows, rs.slice(0, 1), rs.slice(1)];
    }
    const text = tipText(el);
    if (el.classList.contains("tt")) {
      return [readings(text).map(([k, t]) => (k ? [k, t] : t.includes(": ") ? t.split(": ", 2) : ["", t]))];
    }
    return [[["", text]]];
  }

  function overDoc(e) {
    let sel = ".tt[title], .tt[data-tip], .g[title], .g[data-tip], .fn";
    if (noteMode === "num") sel += ", .note";
    if (coloring) sel += ", .app.v, .note.lone";
    const el = e.target.closest?.(sel);
    if (!el) return;
    if (el === tip?.el) {
      clearTimeout(hideTimer);
      return;
    }
    if (pinned) return; // text is being selected in the open tip
    clearTimeout(hideTimer);
    const groups = tipFor(el).filter((g) => g.length);
    if (!groups.length) return;
    const r = el.getBoundingClientRect();
    tip = { groups, el, x: r.left + r.width / 2, top: r.top, bottom: r.bottom };
    tipLeft = tip.x;
    tick().then(() => {
      if (!tipEl || !tip) return;
      const w = tipEl.offsetWidth;
      tipLeft = Math.min(Math.max(8 + w / 2, tip.x), window.innerWidth - 8 - w / 2);
    });
  }

  // the tip stays a moment after the pointer leaves the text, so that it can
  // move into the tip; pressing the mouse in the tip (to select and copy)
  // pins it until a click elsewhere, Escape or scrolling
  let hideTimer = null;
  let pinned = false;

  function closeTip() {
    clearTimeout(hideTimer);
    pinned = false;
    tip = null;
  }

  function hideSoon() {
    clearTimeout(hideTimer);
    if (!pinned) hideTimer = setTimeout(closeTip, 300);
  }

  function outDoc(e) {
    if (tip && !tip.el.contains(e.relatedTarget) && !tipEl?.contains(e.relatedTarget)) hideSoon();
  }

  $effect(() => {
    if (!tip) return;
    const down = (e) => {
      if (pinned && !tipEl?.contains(e.target)) closeTip();
    };
    const key = (e) => e.key === "Escape" && closeTip();
    window.addEventListener("mousedown", down, true);
    window.addEventListener("keydown", key);
    return () => {
      window.removeEventListener("mousedown", down, true);
      window.removeEventListener("keydown", key);
    };
  });

  // ---- context menu: the selected text can be copied, searched or kept
  let menu = $state(null);

  const clip = (s, n = 14) => (s.length > n ? s.slice(0, n) + "…" : s);

  /** The line (lb) of a point: the last marker before it in the document. */
  function lbAt(node) {
    if (!scroller || !node) return null;
    let last = null;
    for (const el of scroller.querySelectorAll("[data-lb]")) {
      const pos = el.compareDocumentPosition(node);
      // the marker comes before `node` (or contains it)
      if (pos & Node.DOCUMENT_POSITION_FOLLOWING || pos & Node.DOCUMENT_POSITION_CONTAINED_BY) last = el;
      else break;
    }
    return last?.dataset.lb ?? scroller.querySelector("[data-lb]")?.dataset.lb ?? null;
  }

  async function copyText(text, withCite, lb) {
    try {
      let out = text;
      if (withCite) {
        const c = await api.citeText(vol.lib, vol.path, vol.book_id, lb, vol.title);
        out = `${text}\n${c}`;
      }
      await navigator.clipboard.writeText(out);
    } catch (e) {
      console.warn("copy", e);
    }
  }

  /** The text of the line / paragraph that starts at a marker. */
  function lineText(lb) {
    if (!lb || !scroller) return "";
    const el = scroller.querySelector(`[data-lb="${CSS.escape(lb)}"]`);
    const block = el?.closest("p, .line, li, td, .div") ?? el?.parentElement;
    return (block?.textContent ?? "").replace(/\s+/g, " ").trim().slice(0, 40);
  }

  /** The browser's own find bar (Ctrl+F), filled with the selected text. */
  async function findInPage(text) {
    // the bar takes the page's selection: make sure it is this text
    if (window.getSelection()?.toString().trim() !== text) window.find?.(text, false, false, true);
    await new Promise((r) => setTimeout(r, 60)); // let the menu close
    try {
      await api.findInPage();
    } catch {
      window.find?.(text, false, false, true);
    }
  }

  function readerMenu(e) {
    if (e.target.closest?.(".tip")) return;
    const s = window.getSelection();
    const inside = s && !s.isCollapsed && scroller?.contains(s.anchorNode) && scroller?.contains(s.focusNode);
    const text = inside ? s.toString().trim() : "";
    e.preventDefault();
    closeTip();
    const lb = inside ? lbAt(s.getRangeAt(0).startContainer) : lbAt(e.target);
    const act = (type) => () => onaction?.({ type, text, lb, vol, excerpt: text ? clip(text, 40) : lineText(lb) });
    const q = clip(text);
    const extEl = e.target.closest?.('a[href^="http://"], a[href^="https://"], a.zim-link[data-link-kind="external"]');
    const extHref = extEl?.getAttribute("href") || extEl?.dataset?.zimHref || "";
    const items = [];
    if (extHref) {
      items.push({ label: "复制链接", action: () => navigator.clipboard.writeText(extHref).catch(() => {}) });
      if (text) items.push("separator");
    }
    if (text) {
      items.push(
        { label: "复制文字", action: () => copyText(text, false, lb) },
        { label: "复制文字（&引用出处）", action: () => copyText(text, true, lb) },
        "separator",
      );
      if (onaction) {
        items.push(
          { label: `全文检索：${q}`, action: act("search") },
          { label: `全文匹配：${q}`, action: act("phrase") },
          { label: `书内检索：${q}`, action: act("book") },
          { label: `标题快搜：${q}`, action: act("quick") },
        );
      }
      items.push({ label: `页内查找：${q}`, action: () => findInPage(text) });
      items.push(
        "separator",
        makePinyinItem(text, act("pinyin")),
        { label: `查词典：${q}`, action: act("dict") },
        "separator",
      );
    }
    if (onaction) {
      items.push({ label: "添加书签", action: act("bookmark") });
      items.push({ label: text ? "添加收藏…" : "添加收藏…（先选中文字）", action: act("clip"), disabled: !text });
      if (text) {
        items.push("separator", { label: "加入笔记…", action: act("note") }, { label: `追加到今日记：${localDate()}`, action: act("diary") });
        const last = getLast();
        if (last) items.push({ label: `追加到最近的笔记：${clip(last.title, 12)}`, action: act("note-last") });
      }
    }
    if (settings.aiOn && text && onai) {
      items.push(
        "separator",
        { label: "发给 AI 助手", action: () => onai({ quote: text, lb, autoSend: false }) },
        { label: "追加到 AI 会话", action: () => onai({ quote: text, lb, append: true, autoSend: false }) },
        { label: "AI 解释选中文字", action: () => onai({ quote: text, lb, prompt: "请用通俗准确的现代汉语解释以上引文，并说明其核心义理与背景。", autoSend: true }) },
        { label: "AI 编写创新方案", action: () => onai({ quote: text, lb, prompt: "请基于以上引文的核心思想与方法，结合现代应用场景，编写一份结构清晰、可落地的创新方案。", autoSend: true }) },
      );
    }
    if (items.length) menu = { x: e.clientX, y: e.clientY, items };
  }

  export function getText() {
    if (!vol?.lines) return "";
    return vol.lines.map((l) => l.parts.map((p) => p.t).join("")).join("\n");
  }

  /** A click on the source link of a clipping goes to the place it came from. */
  function docClick(e) {
    const zimLink = e.target.closest?.("a.zim-link");
    if (zimLink && vol?.kind === "zim") {
      e.preventDefault();
      if (!onjump) return;
      api.resolveZimLink(vol.lib, vol.path, zimLink.dataset.zimHref)
        .then((to) => {
          if (!to.external_url) {
            // Archive-relative links are resolved lazily, then use the same
            // smartlib address format as source links in notes.
            const url = sourceUrl({ lib: to.lib, path: to.path, lb: to.lb });
            zimLink.dataset.src = url;
            return onjump(parseSource(url));
          }
          onexternal?.(to.external_url);
        })
        .catch((err) => onaction?.({ type: "error", text: String(err) }));
      return;
    }
    const extLink = e.target.closest?.("a.ext-link");
    if (extLink?.dataset.extHref) {
      e.preventDefault();
      onexternal?.(extLink.dataset.extHref);
      return;
    }
    const a = e.target.closest?.("a.src");
    if (a) {
      e.preventDefault();
      const to = parseSource(a.dataset.src);
      if (!to || !onjump) return;
      Promise.resolve(onjump(to)).catch((err) => onaction?.({ type: "error", text: String(err) }));
      return;
    }
    const rawA = e.target.closest?.("a[href]");
    const rawHref = rawA?.getAttribute("href") || "";
    if (/^https?:\/\//i.test(rawHref)) {
      e.preventDefault();
      onexternal?.(rawHref);
    }
  }

  function chapterChange(e) {
    const c = vol.chapters[e.target.value];
    e.target.value = "";
    if (c) onchapter(c);
  }
</script>

{#if vol}
  <div class="reader">
    <header>
      <div class="nav">
        <TitleLink title={vol.label ? `${vol.title} ${vol.label}` : vol.title} text={shownTitle} onlocate={vol.kind === "zim" ? null : onlocate} />
        {#if vol.volumes.length}
        <select aria-label={vol.kind !== "cbeta" ? "同一目录的文章" : "卷"} class="vols" value={vol.path} onchange={(e) => onvolume(e.target.value)}>
          {#each vol.volumes as v}
            <option value={v.path} disabled={!v.exists}>{v.label}{v.exists ? "" : "（缺）"}</option>
          {/each}
        </select>
        {/if}
        {#if vol.chapters.length}
          <select aria-label="目次" onchange={chapterChange}>
            <option value="">目次…</option>
            {#each vol.chapters as c, i}
              <option value={i}>{"　".repeat(c.depth)}{c.title}</option>
            {/each}
          </select>
        {/if}
        {#if onaction}
          <button type="button" class="tool-btn" onclick={() => onaction({ type: "tag", vol })} title="为本卷（本篇）加标签（Ctrl+T）" aria-label="加标签"><Icon name="tag" size={16} /></button>
          <button type="button" class="tool-btn" onclick={() => onaction({ type: "props", vol })} title="属性（Ctrl+P）" aria-label="属性"><Icon name="sliders" size={16} /></button>
        {/if}
        {@render extra?.()}
        <span class="grow"></span>
        {#if vol.hits}<span class="muted">本卷命中 {vol.hits} 处</span>{/if}
        <div class="seg" role="group" aria-label="版式">
          <button class:on={mode === "para"} onclick={() => (mode = "para")}>段落</button>
          <button class:on={mode === "lines"} onclick={() => (mode = "lines")}>原书行</button>
        </div>
        <div class="seg" role="group" aria-label="字形">
          <button class:on={!simp} title="按原文的字形显示" onclick={() => update({ readerScript: "orig" })}>原文</button>
          <button class:on={simp} title="显示成简体字（只改显示，不改文件和索引）" onclick={() => update({ readerScript: "simp" })}>简体</button>
        </div>
        {#if mode === "para"}
          <label class="check"><input type="checkbox" bind:checked={showLb} /> 行号</label>
          {#if vol.kind === "cbeta"}
            <label class="check" title="正文用底本（大正藏等）的读法，不用 CBETA 修订的读法；只改显示"><input type="checkbox" bind:checked={baseText} /> 底本读法</label>
            <div class="seg notes" role="group" aria-label="编注显示方式">
              {#each NOTE_MODES as [k, label, help]}
                <button class:on={noteMode === k} title={help} onclick={() => ((shared.noteMode = k), closeTip())}>{label}</button>
              {/each}
            </div>
          {:else if hasFn}
            <div class="seg notes" role="group" aria-label="注释显示">
              <button class:on={noteMode === "none"} title="不显示注释标记" onclick={() => ((shared.noteMode = "none"), closeTip())}>无</button>
              <button class:on={noteMode !== "none"} title="显示（注），鼠标移上去看注释" onclick={() => ((shared.noteMode = noteMode === "none" ? "color" : noteMode), closeTip())}>标注</button>
            </div>
          {/if}
        {/if}
      </div>
    </header>
    <!-- svelte-ignore a11y_mouse_events_have_key_events, a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="scroll" bind:this={scroller} onscroll={(e) => (closeTip(), onScroll(e))} oncontextmenu={readerMenu} onclick={docClick}
      onmouseover={overDoc} onmouseout={outDoc}>
      {#if mode === "para"}
        <article class="doc notes-{noteMode}" class:show-lb={showLb} class:base-text={baseText}>{@html html}</article>
      {:else}
        <div class="lines">
          {#each vol.lines as line}
            <div class="line" data-lb={line.lb}>
              <span class="lbn">{line.lb}</span>
              <span class="txt">{#each line.parts as p}{#if p.hl}<mark>{simp ? simplifyText(p.t) : p.t}</mark>{:else}{simp ? simplifyText(p.t) : p.t}{/if}{/each}</span>
            </div>
          {/each}
        </div>
      {/if}
      {#if vol.attribution}<footer class="attribution">{vol.attribution}</footer>{/if}
    </div>
  </div>
{/if}

{#if tip}
  {@const above = tip.top > 160}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="tip" class:below={!above} bind:this={tipEl} role="tooltip"
    onmouseenter={() => clearTimeout(hideTimer)} onmouseleave={hideSoon} onmousedown={() => (pinned = true)}
    style="left: {tipLeft}px; {above ? `bottom: ${window.innerHeight - tip.top + 10}px` : `top: ${tip.bottom + 10}px`}; --arrow: {tip.x - tipLeft}px">
    <table>
      <tbody>
        {#each tip.groups as rows, g}
          {#if g > 0}<tr class="sep"><td colspan="2"></td></tr>{/if}
          {#each rows as [k, t]}
            <tr><td class="k">{k}{k && t ? "：" : ""}</td><td>{t}</td></tr>
          {/each}
        {/each}
      </tbody>
    </table>
  </div>
{/if}

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} items={menu.items} onclose={() => (menu = null)} />
{/if}

<style>
  .tip {
    position: fixed; z-index: 2000; transform: translateX(-50%); max-width: min(32em, calc(100vw - 16px));
    background: var(--ink); color: var(--paper); border-radius: 6px; padding: 8px 12px;
    font-size: 15px; line-height: 1.6; box-shadow: 0 6px 18px rgba(0, 0, 0, 0.25);
    user-select: text; cursor: text;
  }
  .tip::after {
    content: ""; position: absolute; left: calc(50% + var(--arrow, 0px)); bottom: -6px; transform: translateX(-50%);
    border: 6px solid transparent; border-bottom: none; border-top-color: var(--ink);
  }
  .tip.below::after { bottom: auto; top: -6px; border: 6px solid transparent; border-top: none; border-bottom-color: var(--ink); }
  .tip table { border-collapse: collapse; }
  .tip td { padding: 1px 0; vertical-align: top; }
  .tip td.k { white-space: nowrap; padding-right: 2px; opacity: 0.85; text-align: right; }
  .tip tr.sep td { padding: 0; height: 9px; border-bottom: 1px solid color-mix(in srgb, var(--paper) 35%, transparent); }
  .tip tr.sep + tr td { padding-top: 6px; }
  .reader { display: flex; flex-direction: column; height: 100%; }
  /* title, author and path are in the tab's tooltip */
  header {
    display: flex; align-items: center; gap: 16px; flex-wrap: wrap;
    padding: 8px 24px; border-bottom: 1px solid var(--line); background: var(--paper);
  }
  .grow { flex: 1; }
  .nav { flex: 1; min-width: 0; display: flex; gap: 8px; align-items: center; flex-wrap: wrap; font-size: 14px; }
  .vols { max-width: 16em; }
  .seg { display: inline-flex; }
  .seg button { border-radius: 0; padding: 4px 10px; }
  .seg button:first-child { border-radius: 6px 0 0 6px; }
  .seg button:last-child { border-radius: 0 6px 6px 0; border-left: none; }
  .seg button.on { background: var(--accent); color: var(--paper); border-color: var(--accent); }
  .check { display: flex; gap: 4px; align-items: center; }
  .tool-btn {
    display: inline-flex; align-items: center; gap: 4px;
    padding: 3px 8px; line-height: 1; font-size: 13px;
    border: 1px solid var(--line); border-radius: 5px;
    background: var(--paper); color: var(--accent); cursor: pointer;
  }
  .tool-btn:hover { background: var(--accent-soft); border-color: var(--accent); }
  .scroll { flex: 1; overflow: auto; background: var(--paper); }
  .attribution { margin: 12px 24px 32px; padding: 12px 0; border-top: 1px solid var(--line); color: var(--muted); font-size: 13px; line-height: 1.5; }

  /* original lines */
  .lines { padding: 16px 24px 48px; }
  .line { display: flex; gap: 16px; line-height: var(--reader-line, 2); font-size: var(--reader-size, 19px); font-family: var(--reader-font, inherit); border-radius: 4px; }
  .line:global(.flash) { background: var(--focus); animation: fade 2.5s ease-out 0.5s forwards; }
  .lbn {
    flex: none; width: 64px; text-align: right; font-family: ui-monospace, Consolas, monospace;
    font-size: 12px; color: var(--muted); padding-top: 6px; user-select: none;
  }
  .txt { letter-spacing: 0.04em; }
  @keyframes fade { to { background: transparent; } }

  /* structured view: the HTML comes from core/src/render.rs */
  .doc {
    max-width: 46em; margin: 0 auto; padding: 24px 32px 64px; font-size: var(--reader-size, 19px);
    line-height: var(--reader-line, 2); letter-spacing: 0.04em; font-family: var(--reader-font, inherit);
  }
  .doc :global(mark) { color: var(--hl); background: var(--hl-bg); border-radius: 2px; }
  .doc :global(.div) { margin: 0.2em 0; }
  .doc :global(.head) { font-weight: 700; font-size: 1.08em; margin: 1em 0 0.4em; color: var(--accent); }
  .doc :global(.juan) { font-weight: 700; margin: 0.6em 0; }
  .doc :global(.juan.f-close) { margin-top: 2em; color: var(--muted); }
  .doc :global(.byline) { margin: 0.2em 0 0.2em 6em; color: var(--muted); }
  .doc :global(.docnumber) { margin: 0; font-size: 0.75em; color: var(--muted); }
  .doc :global(p.p) { margin: 0.35em 0; }
  .doc :global(p.quote) { margin: 0.2em 0; padding: 0.1em 0 0.1em 14px; border-left: 3px solid var(--accent-soft); color: var(--muted); }
  .doc :global(a.src) { color: var(--accent); cursor: pointer; text-decoration: underline dotted; }
  .doc :global(a.zim-link), .doc :global(a.ext-link) { color: var(--accent); cursor: pointer; text-decoration: underline; }
  .doc :global(p.r-text-right) { text-align: right; }
  .doc :global(p.t-pre) { white-space: pre; line-height: 1.5; overflow-x: auto; font-size: 0.9em; }
  .doc :global(p.t-dharani) { color: var(--ink); }
  .doc :global(.lg) { margin: 0.5em 0 0.5em 2em; }
  .doc :global(.l) { margin: 0; }
  .doc :global(.caesura) { display: inline-block; width: 1.5em; }
  .doc :global(.inline-note) { font-size: 0.8em; color: var(--muted); }
  .doc :global(.list) { list-style: none; margin: 0.3em 0; padding-left: 1.5em; }
  .doc :global(.table) { border-collapse: collapse; margin: 0.5em 0; font-size: 0.95em; }
  .doc :global(.table td) { border: 1px solid var(--line); padding: 2px 8px; vertical-align: top; }
  .doc :global(.entry) { margin: 0.4em 0; }
  .doc :global(.entry .form) { font-weight: 700; margin-right: 0.5em; }
  .doc :global(.entry .def), .doc :global(.entry .def p) { display: inline; }
  .doc :global(.sp) { margin: 0.3em 0 0.3em 1em; }
  .doc :global(.hi.r-bold) { font-weight: 700; }
  .doc :global(.hi.r-border) { border: 1px solid var(--muted); padding: 0 2px; }
  .doc :global(.hi.r-circle) { border: 1px solid var(--muted); border-radius: 50%; padding: 0 2px; }
  .doc :global(.g) { color: #8a3c6a; font-size: 0.85em; }
  .doc :global(.tt[title]:not([title=""])), .doc :global(.tt[data-tip]:not([data-tip=""])) { border-bottom: 1px dotted var(--muted); }
  .doc :global(.unclear) { color: var(--muted); }
  .doc :global(img.figure) { max-width: 100%; display: block; margin: 0.5em 0; background: white; }
  /* notes by mode: a note point holds .nb-orig and/or .nb-cb */
  .doc :global(.note sup), .doc :global(.note .nb-orig), .doc :global(.note .nb-cb) { display: none; }
  /* footnote mark of an HTML article: the "(注)" is CSS content, so it is
     not copied with the text; the note shows on hover */
  .doc :global(.fn)::before {
    content: "（注）"; color: var(--note); font-weight: 700; font-size: 0.8em; cursor: help; user-select: none;
  }
  .doc :global(.fn:hover)::before { background: var(--accent-soft); border-radius: 3px; }
  .doc.base-text :global(.app.o) { font-size: 0; }
  .doc.base-text :global(.app.o)::after { content: attr(data-orig); font-size: var(--reader-size); color: var(--note); }
  .doc.notes-none :global(.fn) { display: none; }
  .doc.notes-num :global(.note) { cursor: help; }
  .doc.notes-num :global(.note sup) {
    display: inline; color: var(--accent); font-size: 0.85em; font-weight: 700; padding: 0 3px; margin: 0 1px;
    border-radius: 4px; background: var(--accent-soft); vertical-align: super; line-height: 1;
  }
  .doc.notes-num :global(.note:hover sup) { background: var(--accent); color: var(--paper); }
  .doc.notes-orig :global(.note .nb-orig),
  .doc.notes-cb :global(.note .nb-cb),
  .doc.notes-cb :global(.note:not(:has(.nb-cb)) .nb-orig) {
    display: inline; font-size: 0.68em; line-height: 1.4; letter-spacing: 0; color: var(--muted);
    background: var(--panel); border-radius: 3px; padding: 0 3px; margin: 0 2px;
  }
  .doc.notes-color :global(.app.v), .doc.notes-color-orig :global(.app.v) { color: var(--note); cursor: help; }
  .doc.notes-color :global(.app.v:hover), .doc.notes-color-orig :global(.app.v:hover) { background: var(--accent-soft); border-radius: 2px; }
  .doc.notes-color :global(.note.lone)::before, .doc.notes-color-orig :global(.note.lone)::before {
    content: "✱"; font-size: 0.55em; vertical-align: super; color: var(--note); opacity: 0.6; cursor: help;
  }
  /* the jump marker must not open a line of its own between two paragraphs */
  .doc :global(.lb.flash) { position: relative; }
  .doc :global(.lb.flash)::before {
    content: "▼"; color: var(--hl); font-size: 0.55em; position: absolute; left: -0.5em; top: -1.3em;
  }
  .doc :global(.lb.flash) { scroll-margin: 40vh; }
  .doc.show-lb :global(.lb)::before {
    content: attr(data-lb); font-family: ui-monospace, Consolas, monospace; font-size: 10px; color: var(--muted);
    vertical-align: super; margin: 0 2px; letter-spacing: 0;
  }
  .doc.show-lb :global(.lb.flash)::before { color: var(--hl); font-weight: 700; }
</style>
