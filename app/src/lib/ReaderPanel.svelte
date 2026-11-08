<script>
  // The content of one reading tab: one document (a CBETA volume or an HTML
  // article; a tab is identified by `<library>|<path>`). `p` is a reactive object owned by the workspace, updated
  // when the same volume is opened again (new target / query).
  import { untrack, tick } from "svelte";
  import * as api from "./api.js";
  import { setLast } from "./clipTarget.js";
  import Reader from "./Reader.svelte";

  let { p, onopen, onprogress, ontitle, onlocate = null, onaction = null } = $props();

  let vol = $state(null);
  let error = $state("");
  let localTarget = $state(null);
  let localNonce = $state(0);

  // internal libraries: the Markdown source can be edited in the tab
  let editing = $state(false);
  let draft = $state("");
  let saving = $state(false);
  let reloads = $state(0);
  let seen = ""; // the note the editor was last opened for

  // (re)load when the query of the tab changes; the path never changes
  $effect(() => {
    const { lib, bookId, path, query, phrase } = p;
    const _r = reloads;
    const _x = p.reload; // the note changed outside (a clipping was added)
    api
      .openNode(lib, bookId, path, query || null, !!phrase)
      .then((v) => {
        vol = v;
        error = "";
        // a new, empty note opens in the editor
        if (v.kind === "notes" && !v.raw && seen !== p.lib + "|" + v.path) startEdit(v);
        seen = p.lib + "|" + v.path;
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

  function startEdit(v = vol) {
    draft = v.raw ?? "";
    editing = true;
    setLast({ lib: p.lib, id: v.path, title: v.title }); // the note clippings go to by default
  }

  // the workspace asks whether this note is being edited, and passes a clipping to the editor
  $effect(() => {
    p.editing = editing;
  });
  $effect(() => {
    const a = p.append;
    if (!a) return;
    untrack(() => {
      if (editing) draft = draft.replace(/\s+$/, "") + "\n\n" + a.text.trim() + "\n";
    });
    p.append = null;
  });

  async function saveEdit() {
    saving = true;
    try {
      await api.saveNote(p.lib, p.path, draft);
      if (vol) setLast({ lib: p.lib, id: p.path, title: vol.title });
      editing = false;
      reloads += 1;
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }

  function cancelEdit() {
    editing = false;
  }

  /** Pictures pasted or dropped into the editor are kept and shown as `![](attach:…)`. */
  let editorEl = $state();
  async function addPictures(files) {
    for (const f of files) {
      try {
        const name = await api.noteAttach(p.lib, await f.arrayBuffer(), f.type);
        const at = editorEl?.selectionStart ?? draft.length;
        const end = editorEl?.selectionEnd ?? at;
        const text = `![](attach:${name})\n`;
        draft = draft.slice(0, at) + text + draft.slice(end);
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

  function editorKey(e) {
    if ((e.ctrlKey || e.metaKey) && e.key === "s") {
      e.preventDefault();
      saveEdit();
    } else if (e.key === "Escape") cancelEdit();
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
{:else if vol && editing}
  <div class="editor">
    <div class="bar">
      <strong>{vol.title}</strong>
      <span class="muted">Markdown · Ctrl+S 保存</span>
      <span class="grow"></span>
      <button onclick={cancelEdit}>取消</button>
      <button class="primary" disabled={saving} onclick={saveEdit}>保存</button>
    </div>
    <!-- svelte-ignore a11y_autofocus -->
    <textarea bind:value={draft} bind:this={editorEl} onkeydown={editorKey} onpaste={editorPaste} ondrop={editorDrop} ondragover={(e) => e.preventDefault()} autofocus spellcheck="false"
      placeholder="# 标题&#10;正文……（支持 # 标题、- 列表、**粗体**、*斜体*；可以直接粘贴或拖入图片）"></textarea>
  </div>
{:else if vol}
  {#snippet editButton()}
    {#if vol.kind === "notes"}<button title="编辑这篇笔记" onclick={() => startEdit()}>编辑</button>{/if}
  {/snippet}
  <Reader
    extra={editButton}
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
  .bar { display: flex; gap: 8px; align-items: center; padding: 8px 12px; border-bottom: 1px solid var(--line); }
  .grow { flex: 1; }
  textarea {
    flex: 1; border: none; resize: none; padding: 14px 18px; outline: none;
    font: 15px/1.7 ui-monospace, Consolas, "Microsoft YaHei", monospace; background: var(--paper); color: inherit;
  }
  .loading { padding: 24px; }
</style>
