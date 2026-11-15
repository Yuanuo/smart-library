<script>
  // The content of one reading tab: one document (a CBETA volume or an HTML
  // article; a tab is identified by `<library>|<path>`). `p` is a reactive object owned by the workspace, updated
  // when the same volume is opened again (new target / query).
  //
  // The notes of internal libraries have no separate reading and editing:
  // they open in the visual editor (it is the reader too) and are saved a
  // moment after every change. Mind maps open in the map component.
  import { onDestroy, untrack, tick } from "svelte";
  import * as api from "./api.js";
  import { setLast } from "./clipTarget.js";
  import { settings, update, isAiReady } from "./settings.svelte.js";
  import Reader from "./Reader.svelte";
  import TitleLink from "./TitleLink.svelte";
  import ContextMenu from "./ContextMenu.svelte";
  import { makePinyinItem } from "./pinyinMenu.svelte.js";
  import { localDate } from "./diary.js";
  import Icon from "./Icon.svelte";
  import SnapshotDialog from "./SnapshotDialog.svelte";

  const loadMindMap = () => import("./MindMapView.svelte");
  const loadWhiteboard = () => import("./WhiteboardView.svelte");

  let { p, onopen, onprogress, ontitle, onlocate = null, onaction = null, onai = null, ondocstate = null, getNodePath = null } = $props();

  let vol = $state(null);
  let error = $state("");
  let localTarget = $state(null);
  let localNonce = $state(0);

  let readerRef = $state();
  let mindRef = $state();
  let boardRef = $state();
  let externalPrompt = $state(null);
  let snapshotOpen = $state(false);

  async function openExternal(url, mode = "window", remember = false) {
    externalPrompt = null;
    try {
      if (mode === "browser") await api.openExternalUrl(url);
      else await api.openExternalWindow(url, !!settings.aiOn);
      if (remember) update({ externalLink: mode });
    } catch (err) {
      onaction?.({ type: "error", text: String(err) });
    }
  }

  function handleExternal(url) {
    if (!url) return;
    const mode = settings.externalLink || "window";
    if (mode === "window" || mode === "browser") {
      openExternal(url, mode);
    } else {
      externalPrompt = url;
    }
  }

  function sendToAi({ text = "", quote = "", lb = null, prompt = "", toggle = false, append = false } = {}) {
    const title = p.retitled ?? (vol?.label ? `${vol.title} ${vol.label}` : vol?.title);
    const source = [title, lb].filter(Boolean).join(" · ");
    onai?.({
      toggle,
      append,
      text: quote || text || "",
      lb,
      source,
      prompt,
      sourceDoc: vol ? { lib: p.lib, bookId: p.bookId, path: p.path, title: p.retitled ?? vol.title } : null,
    });
  }

  // `raw` comes only with what can be edited here: the notes of internal
  // libraries and the editable files of folder libraries in read-write mode
  const isNote = (v) => v?.raw != null && v.content_type !== "mindmap" && v.content_type !== "whiteboard";

  function getDocText() {
    if (!vol) return "";
    if (vol.content_type === "mindmap") return mindRef?.getText?.() || vol.raw || "";
    if (vol.content_type === "whiteboard") return boardRef?.getText?.() || "";
    if (isNote(vol)) return draft || "";
    return readerRef?.getText?.() || "";
  }

  function getCurrentRawContent() {
    if (!vol || vol.raw == null) return null;
    if (vol.content_type === "mindmap") return mindRef?.getRaw?.() ?? vol.raw;
    if (vol.content_type === "whiteboard") return boardRef?.getRaw?.() ?? vol.raw;
    return draft;
  }

  async function handleEditDoc({ mode, text }) {
    if (!vol || vol.raw == null || !text?.trim()) return;
    const raw = getCurrentRawContent();
    await api.createSnapshot(p.lib, p.path, "ai", "AI 修改前", raw).catch(() => {});
    if (vol.content_type === "mindmap") {
      await mindRef?.applyAi?.({ mode, text });
    } else if (vol.content_type === "whiteboard") {
      await boardRef?.applyAi?.({ mode, text });
    } else if (isNote(vol)) {
      if (mode === "replace") {
        draft = text.trim() + "\n";
      } else {
        const base = draft.replace(/\s+$/, "");
        draft = (base ? base + "\n\n" : "") + text.trim() + "\n";
      }
      ver += 1;
      changed(draft);
    }
  }

  $effect(() => {
    if (!vol) {
      ondocstate?.(null);
      return;
    }
    ondocstate?.({
      lib: p.lib,
      path: p.path,
      bookId: p.bookId,
      title: p.retitled ?? vol.title,
      contentType: vol.content_type || "markdown",
      writable: vol.raw != null,
      getText: getDocText,
      editDoc: vol.raw != null ? handleEditDoc : null,
    });
  });

  // ---- (re)load when the query of the tab changes; the path never changes
  let draft = $state(""); // the note's Markdown
  let ver = $state(0); // +1: the editor reads `draft` again
  let loaded = false; // the draft came from the note
  let seenReload = -1;

  $effect(() => {
    const { lib, bookId, path, query, phrase } = p;
    const reload = p.reload; // the note changed outside (a clipping was added)
    api
      .openNode(lib, bookId, path, query || null, !!phrase)
      .then((v) => {
        vol = v;
        error = "";
        // the draft is read once, and again when the note changed outside; a new
        // search or jump in the same note must not take the editor away from the user
        if (isNote(v) && (!loaded || reload !== seenReload)) {
          draft = v.raw ?? "";
          loaded = true;
          seenReload = reload;
          editSeq = 0;
          savedSeq = 0;
          ver += 1;
        }
        // the tab shows the title; its tooltip also the id, author and path
        const t = v.label ? `${v.title} ${v.label}` : v.title;
        const who = [v.book_id, v.author].filter(Boolean).join(" · ");
        ontitle?.(t, [t, who, v.path].filter(Boolean).join("\n"), { author: v.author || "" });
      })
      .catch((e) => (error = String(e)));
  });

  // a jump requested from outside (search hit, tree, restore) wins over a
  // chapter jump inside the tab
  $effect(() => {
    localTarget = p.target ?? (p.query && vol ? vol.first_hit_lb : null);
    localNonce = p.nonce ?? 0;
  });

  // ---- notes: saved a moment after every change
  let editSeq = 0;
  let savedSeq = 0;
  let status = $state("");
  let saveTimer = null;

  function changed(md) {
    draft = md;
    editSeq += 1;
    status = "未保存";
    clearTimeout(saveTimer);
    saveTimer = setTimeout(saveNow, 1000);
  }

  async function saveNow() {
    clearTimeout(saveTimer);
    if (editSeq === savedSeq || !vol) return;
    const targetSeq = editSeq;
    const text = draft;
    try {
      await api.saveNote(p.lib, p.path, text);
      if (vol.kind === "notes") setLast({ lib: p.lib, id: p.path, title: vol.title }); // the note clippings go to by default
      if (savedSeq < targetSeq) savedSeq = targetSeq;
      if (editSeq === savedSeq) status = "已保存";
    } catch (e) {
      if (editSeq === targetSeq) status = "保存失败：" + String(e);
    }
  }

  onDestroy(() => {
    clearTimeout(saveTimer);
    if (editSeq !== savedSeq && vol) api.saveNote(p.lib, p.path, draft).catch(() => {});
  });

  // the workspace asks whether this note is open in its editor (then a clipping goes into it)
  $effect(() => {
    p.editing = isNote(vol);
  });
  $effect(() => {
    const a = p.append;
    if (!a) return;
    untrack(() => {
      if (isNote(vol)) {
        draft = draft.replace(/\s+$/, "") + "\n\n" + a.text.trim() + "\n";
        ver += 1; // the editor reads the text again
        changed(draft);
      }
    });
    p.append = null;
  });

  // ---- the editor: visual (Milkdown) or the Markdown source
  // loaded when first needed: the editor library is big
  const loadEditor = () => import("./NoteEditor.svelte");
  let visual = $state(true);
  try {
    visual = localStorage.getItem("note.editor") !== "source";
  } catch {}
  function setVisual(v) {
    visual = v;
    ver += 1;
    try {
      localStorage.setItem("note.editor", v ? "visual" : "source");
    } catch {}
  }
  const keepPicture = async (file) => api.noteAttach(p.lib, await file.arrayBuffer(), file.type, p.path);

  /** Pictures pasted or dropped into the source editor are kept and shown as `![](attach:…)`. */
  let editorEl = $state();
  async function addPictures(files) {
    for (const f of files) {
      try {
        const name = await keepPicture(f);
        const at = editorEl?.selectionStart ?? draft.length;
        const end = editorEl?.selectionEnd ?? at;
        const text = `![](attach:${name})\n`;
        draft = draft.slice(0, at) + text + draft.slice(end);
        changed(draft);
        await tick();
        editorEl?.setSelectionRange(at + text.length, at + text.length);
      } catch (e) {
        error = String(e);
      }
    }
  }
  const pictures = (dt) => [...(dt?.files ?? [])].filter((f) => f.type.startsWith("image/"));
  function editorPaste(e) {
    const files = pictures(e.clipboardData);
    if (files.length) {
      e.preventDefault();
      addPictures(files);
    }
  }
  function editorDrop(e) {
    const files = pictures(e.dataTransfer);
    if (files.length) {
      e.preventDefault();
      addPictures(files);
    }
  }

  function noteKey(e) {
    if ((e.ctrlKey || e.metaKey) && e.key === "s") {
      e.preventDefault();
      saveNow();
    }
  }

  // ---- the menu of selected text in a note (the same as the reader's)
  let menu = $state(null);

  const clip = (s, n = 14) => (s.length > n ? s.slice(0, n) + "…" : s);

  async function findInPage(text) {
    if (window.getSelection()?.toString().trim() !== text) window.find?.(text, false, false, true);
    await new Promise((r) => setTimeout(r, 60));
    try {
      await api.findInPage();
    } catch {
      window.find?.(text, false, false, true);
    }
  }

  function noteMenu({ x, y, text, lb }) {
    const act = (type) => () => onaction?.({ type, text, lb, vol, excerpt: clip(text, 40) });
    const q = clip(text);
    const items = [{ label: "复制文字", action: () => navigator.clipboard.writeText(text).catch(() => {}) }, "separator"];
    if (onaction) {
      items.push(
        { label: `全文检索：${q}`, action: act("search") },
        { label: `全文匹配：${q}`, action: act("phrase") },
        { label: `篇内检索：${q}`, action: act("book") },
        { label: `标题快搜：${q}`, action: act("quick") },
      );
    }
    items.push({ label: `页内查找：${q}`, action: () => findInPage(text) });
    if (onaction) {
      items.push(
        "separator",
        makePinyinItem(text, act("pinyin")),
        { label: `查词典：${q}`, action: act("dict") },
      );
    }
    if (onaction) {
      items.push(
        "separator",
        { label: "加入笔记…", action: act("note") },
        { label: `追加到今日记：${localDate()}`, action: act("diary") },
        { label: "添加书签", action: act("bookmark") },
        { label: "添加收藏…", action: act("clip") },
      );
    }
    if (settings.aiOn && text) {
      items.push(
        "separator",
        { label: "发给 AI 助手", action: () => sendToAi({ quote: text, lb, autoSend: false }) },
        { label: "追加到 AI 会话", action: () => sendToAi({ quote: text, lb, append: true, autoSend: false }) },
        { label: "AI 解释选中文字", action: () => sendToAi({ quote: text, lb, prompt: "请解释以上文字的核心含义与要点。", autoSend: true }) },
        { label: "AI 润色 / 续写", action: () => sendToAi({ quote: text, lb, prompt: "请基于以上内容进行润色并自然续写。", autoSend: true }) },
        { label: "AI 编写创新方案", action: () => sendToAi({ quote: text, lb, prompt: "请根据以上文字的核心要点编写一份结构化的创新方案。", autoSend: true }) },
      );
    }
    menu = { x, y, items };
  }

  function chapter(c) {
    if (c.path === p.path) {
      localTarget = c.lb;
      localNonce += 1;
      p.quiet = false;
    } else {
      onopen({ lib: p.lib, bookId: p.bookId, path: c.path, lb: c.lb });
    }
  }
</script>

{#snippet aiBtn()}
  {#if vol?.raw != null}
    <button type="button" class="tool-btn" onclick={() => (snapshotOpen = true)} title="文档快照（查看、创建或还原历史版本）" aria-label="文档快照">
      <Icon name="snapshot" size={16} />
    </button>
  {/if}
  {#if settings.aiOn && onai}
    <button type="button" class="ai-btn" onclick={() => sendToAi({ toggle: true })} title="打开 / 收起右侧 AI 助手栏（F1）">
      <Icon name="sparkles" size={16} /> AI
    </button>
  {/if}
{/snippet}

{#if error}
  <p class="error">{error}</p>
{:else if vol}
  {#if vol.content_type === "mindmap"}
    {#key vol}
      {#await loadMindMap()}
        <p class="muted loading">载入脑图…</p>
      {:then m}
        <m.default
          bind:this={mindRef}
          {vol}
          title={p.retitled ?? vol.title}
          {onlocate}
          ontag={onaction ? () => onaction({ type: "tag", vol }) : null}
          onprops={onaction ? () => onaction({ type: "props", vol }) : null}
          lib={p.lib}
          path={p.path}
          onjump={onopen}
          onexternal={handleExternal}
          extra={aiBtn}
          onai={(req) => sendToAi(req)} />
      {:catch e}
        <p class="error">{String(e)}</p>
      {/await}
    {/key}
  {:else if vol.content_type === "whiteboard"}
    {#key vol}
      {#await loadWhiteboard()}
        <p class="muted loading">载入白板…</p>
      {:then m}
        <m.default
          bind:this={boardRef}
          {vol}
          title={p.retitled ?? vol.title}
          {onlocate}
          ontag={onaction ? () => onaction({ type: "tag", vol }) : null}
          onprops={onaction ? () => onaction({ type: "props", vol }) : null}
          lib={p.lib}
          path={p.path}
          extra={aiBtn} />
      {:catch e}
        <p class="error">{String(e)}</p>
      {/await}
    {/key}
  {:else if isNote(vol)}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="editor" onkeydown={noteKey}>
      <div class="bar">
        <TitleLink title={p.retitled ?? vol.title} {onlocate} />
        {#if onaction}
          <button type="button" class="tool-btn" onclick={() => onaction({ type: "tag", vol })} title="为本篇加标签（Ctrl+T）" aria-label="加标签"><Icon name="tag" size={16} /></button>
          <button type="button" class="tool-btn" onclick={() => onaction({ type: "props", vol })} title="属性（Ctrl+P）" aria-label="属性"><Icon name="sliders" size={16} /></button>
        {/if}
        {@render aiBtn()}
        <span class="muted status">{status}</span>
        <span class="grow"></span>
        <div class="seg" role="group" aria-label="编辑方式">
          <button class:on={visual} onclick={() => !visual && setVisual(true)} title="所见即所得">排版</button>
          <button class:on={!visual} onclick={() => visual && setVisual(false)} title="直接编辑 Markdown 源码">源码</button>
        </div>
      </div>
      <div class="body">
        {#key `${ver}-${isAiReady()}`}
          {#if visual}
            {#await loadEditor()}
              <p class="muted loading">载入编辑器…</p>
            {:then m}
              <m.default
                value={draft}
                lib={p.lib}
                onchange={changed}
                onpicture={keepPicture}
                query={p.query}
                phrase={!!p.phrase}
                target={localTarget}
                nonce={localNonce}
                onprogress={(lb) => onprogress(lb, p.mode)}
                onjump={onopen}
                onexternal={handleExternal}
                onmenu={noteMenu} />
            {:catch e}
              <p class="error">{String(e)}</p>
            {/await}
          {:else}
            <textarea bind:value={draft} bind:this={editorEl} oninput={() => changed(draft)} onpaste={editorPaste} ondrop={editorDrop} ondragover={(e) => e.preventDefault()} spellcheck="false"
              placeholder="# 标题&#10;正文……（支持 # 标题、- 列表、**粗体**、*斜体*；可以直接粘贴或拖入图片）"></textarea>
          {/if}
        {/key}
      </div>
    </div>
    {#if menu}
      <ContextMenu x={menu.x} y={menu.y} items={menu.items} onclose={() => (menu = null)} />
    {/if}
  {:else}
    <Reader
      bind:this={readerRef}
      {vol}
      target={localTarget}
      nonce={localNonce}
      quiet={!!p.quiet}
      bind:mode={p.mode}
      onvolume={(path) => onopen({ lib: p.lib, bookId: p.bookId, path })}
      onchapter={chapter}
      {onlocate}
      {onaction}
      onjump={onopen}
      onexternal={handleExternal}
      extra={aiBtn}
      onai={(req) => sendToAi(req)}
      onprogress={(lb) => onprogress(lb, p.mode)} />
  {/if}
{:else}
  <p class="loading muted">载入中…</p>
{/if}

{#if snapshotOpen && vol?.raw != null}
  <SnapshotDialog
    lib={p.lib}
    docId={p.path}
    title={p.retitled ?? vol.title}
    nodePath={getNodePath?.(vol) || [p.lib, p.retitled ?? vol.title].filter(Boolean).join("/")}
    getCurrentContent={getCurrentRawContent}
    onrestored={() => {
      clearTimeout(saveTimer);
      editSeq = 0;
      savedSeq = 0;
      loaded = false;
      p.reload = (p.reload ?? 0) + 1;
    }}
    onclose={() => (snapshotOpen = false)} />
{/if}

{#if externalPrompt}
  <div class="external-backdrop" role="presentation" onmousedown={(e) => { if (e.target === e.currentTarget) externalPrompt = null; }}>
    <div class="external-dialog" role="dialog" aria-modal="true" aria-label="打开外部链接">
      <h2>打开外部链接？</h2>
      <p>这是一个外部网页链接，是否在新窗口中打开？</p>
      <p class="external-url">{externalPrompt}</p>
      <div class="external-actions">
        <button onclick={() => (externalPrompt = null)}>取消</button>
        <button onclick={() => openExternal(externalPrompt, "browser")}>系统浏览器</button>
        <button onclick={() => openExternal(externalPrompt, "window")}>新窗口打开</button>
        <button class="primary" onclick={() => openExternal(externalPrompt, "window", true)}>新窗口打开并不再询问</button>
      </div>
      <p class="muted">之后可在设置中修改外部链接打开方式。</p>
    </div>
  </div>
{/if}

<style>
  .ai-btn, .tool-btn {
    display: inline-flex; align-items: center; gap: 4px;
    padding: 3px 8px; line-height: 1; font-size: 13px;
    border: 1px solid var(--line); border-radius: 5px;
    background: var(--paper); color: var(--accent); cursor: pointer;
  }
  .ai-btn:hover, .tool-btn:hover { background: var(--accent-soft); border-color: var(--accent); }
  .error { padding: 24px; color: var(--hl); }
  .editor { display: flex; flex-direction: column; height: 100%; }
  .bar { display: flex; gap: 8px; align-items: center; padding: 6px 12px; border-bottom: 1px solid var(--line); }
  .status { font-size: 12px; }
  .grow { flex: 1; }
  .body { flex: 1; min-height: 0; display: flex; flex-direction: column; }
  .seg { display: flex; }
  .seg button { border-radius: 0; margin-left: -1px; padding: 2px 12px; }
  .seg button:first-child { border-radius: 6px 0 0 6px; margin-left: 0; }
  .seg button:last-child { border-radius: 0 6px 6px 0; }
  .seg button.on { background: var(--accent); border-color: var(--accent); color: var(--paper); }
  textarea {
    flex: 1; border: none; resize: none; padding: 14px 18px; outline: none;
    font: 15px/1.7 ui-monospace, Consolas, "Microsoft YaHei", monospace; background: var(--paper); color: inherit;
  }
  .loading { padding: 24px; }
  .external-backdrop {
    position: fixed; inset: 0; z-index: 950; background: rgba(0, 0, 0, 0.35);
    display: flex; align-items: center; justify-content: center;
  }
  .external-dialog {
    width: min(480px, calc(100vw - 32px)); background: var(--paper); border-radius: 10px;
    padding: 18px 22px; box-shadow: 0 12px 36px rgba(0, 0, 0, 0.25);
  }
  .external-dialog h2 { margin: 0 0 8px; font-size: 18px; color: var(--accent); }
  .external-dialog p { margin: 6px 0; font-size: 14px; }
  .external-url {
    padding: 8px 10px; background: var(--bg); border-radius: 6px; font-family: monospace;
    font-size: 12px; word-break: break-all; max-height: 84px; overflow-y: auto;
  }
  .external-actions { display: flex; justify-content: flex-end; gap: 8px; flex-wrap: wrap; margin-top: 12px; }
</style>
