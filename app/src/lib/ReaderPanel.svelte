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
  import Reader from "./Reader.svelte";
  import MindMapView from "./MindMapView.svelte";
  import WhiteboardView from "./WhiteboardView.svelte";
  import ContextMenu from "./ContextMenu.svelte";
  import Icon from "./Icon.svelte";

  let { p, onopen, onprogress, ontitle, onlocate = null, onaction = null } = $props();

  let vol = $state(null);
  let error = $state("");
  let localTarget = $state(null);
  let localNonce = $state(0);

  const isNote = (v) => v?.kind === "notes" && v.content_type !== "mindmap" && v.content_type !== "whiteboard";

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
          dirty = false;
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
  let dirty = false;
  let status = $state("");
  let saveTimer = null;

  function changed(md) {
    draft = md;
    dirty = true;
    status = "未保存";
    clearTimeout(saveTimer);
    saveTimer = setTimeout(saveNow, 1000);
  }

  async function saveNow() {
    clearTimeout(saveTimer);
    if (!dirty || !vol) return;
    dirty = false;
    const text = draft;
    try {
      await api.saveNote(p.lib, p.path, text);
      setLast({ lib: p.lib, id: p.path, title: vol.title }); // the note clippings go to by default
      status = "已保存";
    } catch (e) {
      dirty = true;
      status = "保存失败：" + String(e);
    }
  }

  onDestroy(() => {
    clearTimeout(saveTimer);
    if (dirty && vol) api.saveNote(p.lib, p.path, draft).catch(() => {});
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
  const keepPicture = async (file) => api.noteAttach(p.lib, await file.arrayBuffer(), file.type);

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
        { label: `全文检索（精确检索）：${q}`, action: act("phrase") },
        { label: `全文检索（检索本篇）：${q}`, action: act("book") },
        { label: `快捷检索（经名、作译者等）：${q}`, action: act("quick") },
      );
    }
    items.push({ label: `页内查找：${q}`, action: () => findInPage(text) });
    if (onaction) {
      items.push(
        "separator",
        { label: "加入笔记…", action: act("note") },
        { label: "发送到今日记", action: act("diary") },
        { label: "添加书签", action: act("bookmark") },
        { label: "添加收藏…", action: act("clip") },
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

{#if error}
  <p class="error">{error}</p>
{:else if vol && vol.content_type === "mindmap"}
  {#key vol}
    <MindMapView {vol} lib={p.lib} path={p.path} onjump={onopen} />
  {/key}
{:else if vol && vol.content_type === "whiteboard"}
  {#key vol}
    <WhiteboardView {vol} lib={p.lib} path={p.path} />
  {/key}
{:else if vol && isNote(vol)}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="editor" onkeydown={noteKey}>
    <div class="bar">
      <strong>{vol.title}</strong>
      <span class="muted status">{status}</span>
      <span class="grow"></span>
      <div class="seg" role="group" aria-label="编辑方式">
        <button class:on={visual} onclick={() => !visual && setVisual(true)} title="所见即所得">排版</button>
        <button class:on={!visual} onclick={() => visual && setVisual(false)} title="直接编辑 Markdown 源码">源码</button>
      </div>
      {#if onaction}<button class="icon" onclick={() => onaction({ type: "tag", vol })} title="为本篇加标签" aria-label="加标签"><Icon name="tag" size={20} /></button>{/if}
      {#if onlocate}<button class="icon" onclick={onlocate} title="在目录树中定位" aria-label="在目录树中定位"><Icon name="locate" size={20} /></button>{/if}
    </div>
    <div class="body">
      {#key ver}
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
{:else if vol}
  <Reader
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
    onprogress={(lb) => onprogress(lb, p.mode)} />
{:else}
  <p class="loading muted">载入中…</p>
{/if}

<style>
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
  .icon {
    display: inline-flex; align-items: center; justify-content: center; border: none; background: none;
    padding: 2px 4px; color: var(--accent); border-radius: 6px;
  }
  .icon:hover { background: var(--accent-soft); }
  textarea {
    flex: 1; border: none; resize: none; padding: 14px 18px; outline: none;
    font: 15px/1.7 ui-monospace, Consolas, "Microsoft YaHei", monospace; background: var(--paper); color: inherit;
  }
  .loading { padding: 24px; }
</style>
