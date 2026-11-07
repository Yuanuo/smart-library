<script>
  // The tree panel: one root per bound knowledge library, its tree loaded
  // when first opened. Right-click a library for its actions, or the empty
  // panel to bind a library.
  import { tick } from "svelte";
  import * as api from "./api.js";
  import Tree from "./Tree.svelte";
  import ContextMenu from "./ContextMenu.svelte";
  import NameDialog from "./NameDialog.svelte";
  import NoteDialog from "./NoteDialog.svelte";
  import Icon from "./Icon.svelte";
  import * as workspace from "./workspace.svelte.js";
  import { ask } from "@tauri-apps/plugin-dialog";

  let { libraries, progress, onopen, onbind, onrebind, onunbind, onindex, onsearchfrom } = $props();

  let nameDlg = $state(null); // { title, label, value, okLabel, onok }
  let noteDlg = $state(null); // { position, defaultLib }
  let selected = $state(null); // { lib, key }: the last note opened from the tree

  const isNotes = (id) => libraries.find((l) => l.id === id)?.kind === "notes";
  const parentKey = (key) => key.slice(0, key.lastIndexOf("/"));

  /** Reload one level of a library (`key` null: its top), keeping what was
   * loaded below the notes that are still there. */
  async function refresh(lib, key) {
    const fresh = key ? await api.folderChildren(lib, nodeAt(lib, key)?.rel ?? "") : await api.tree(lib);
    const old = key ? (nodeAt(lib, key)?.children ?? []) : (nodes[lib] ?? []);
    const byRel = new Map(old.map((n) => [n.rel, n]));
    for (const n of fresh) {
      const o = byRel.get(n.rel);
      if (o && o.children.length) {
        n.children = o.children;
        n.lazy = false;
      }
    }
    if (key) {
      const n = nodeAt(lib, key);
      if (n) {
        n.children = fresh;
        n.lazy = false;
        (expanded[lib] ??= {})[key] = true;
      }
    } else {
      nodes[lib] = fresh;
      open[lib] = true;
    }
  }

  const guard = (lib, f) => async (...a) => {
    try {
      await f(...a);
    } catch (e) {
      errors[lib] = String(e);
      throw e;
    }
  };

  /** Titles from the library down to the node at `key`, for the dialog. */
  function pathOf(lib, key) {
    const parts = [libraries.find((l) => l.id === lib)?.title ?? ""];
    let list = nodes[lib] ?? [];
    let k = "";
    for (const i of (key ?? "").split("/").filter(Boolean)) {
      const n = list[Number(i)];
      if (!n) break;
      parts.push(n.title);
      list = n.children;
    }
    return parts.join("/");
  }

  function newNote(lib, parent, key) {
    noteDlg = { position: pathOf(lib, key), defaultLib: lib };
  }

  /** Add the note, show it in the tree and open it. */
  async function addNote(defaultLib, position, title) {
    const r = await api.createNoteAt(defaultLib, position, title);
    // load the way down to the new note (every level on its chain)
    await refresh(r.lib, null);
    let key = "";
    let list = nodes[r.lib];
    for (const rel of r.chain.slice(0, -1)) {
      const i = list.findIndex((n) => n.rel === rel);
      if (i < 0) break;
      key += `/${i}`;
      await refresh(r.lib, key);
      list = nodeAt(r.lib, key).children;
    }
    const idx = list.findIndex((n) => n.rel === r.id);
    if (idx >= 0) {
      current = { lib: r.lib, key: `${key}/${idx}` };
      selected = current;
    }
    await tick();
    panel?.querySelector(`[data-lib="${r.lib}"] [data-key="${current?.key}"]`)?.scrollIntoView({ block: "center" });
    await workspace.openVolume({ lib: r.lib, path: r.id });
  }

  function addNoteHere() {
    const sel = selected && isNotes(selected.lib) ? selected : null;
    const lib = sel?.lib ?? libraries.find((l) => l.kind === "notes")?.id;
    if (!lib) return;
    newNote(lib, null, sel?.key ?? null);
  }

  function renameNote(lib, node, key) {
    nameDlg = {
      title: "重命名笔记",
      label: "标题",
      value: node.title,
      okLabel: "确定",
      onok: guard(lib, async (title) => {
        await api.renameNote(lib, node.rel, title);
        workspace.retitle(lib, node.rel, title);
        await refresh(lib, parentKey(key) || null);
      }),
    };
  }

  async function deleteNote(lib, node, key) {
    const more = node.lazy || node.children.length ? "\n它下面的笔记会一起删除。" : "";
    const ok = await ask(`删除笔记「${node.title}」？${more}\n删除后不能恢复。`, {
      title: "删除笔记",
      kind: "warning",
      okLabel: "删除",
      cancelLabel: "取消",
    });
    if (!ok) return;
    try {
      await api.deleteNote(lib, node.rel);
      workspace.closeDocument(lib, node.rel);
      if (selected?.lib === lib && selected.key.startsWith(key)) selected = null;
      await refresh(lib, parentKey(key) || null);
    } catch (e) {
      errors[lib] = String(e);
    }
  }

  function canShift(lib, node, key, by) {
    const pk = parentKey(key);
    const siblings = pk ? nodeAt(lib, pk)?.children : nodes[lib];
    const idx = siblings?.findIndex((n) => n.rel === node.rel) ?? -1;
    return idx >= 0 && idx + by >= 0 && idx + by < siblings.length;
  }

  async function shiftNote(lib, node, key, by) {
    try {
      const pk = parentKey(key);
      const siblings = pk ? nodeAt(lib, pk).children : nodes[lib];
      const idx = siblings.findIndex((n) => n.rel === node.rel);
      if (idx + by < 0 || idx + by >= siblings.length) return; // already first / last
      const parent = pk ? nodeAt(lib, pk).rel : null;
      await api.moveNote(lib, node.rel, parent, idx + by);
      await refresh(lib, pk || null);
    } catch (e) {
      errors[lib] = String(e);
    }
  }

  /** A node as a search range: the books / articles below it, or (lazy
   * libraries) its `rel` for the backend to expand. */
  function withinOf(l, node) {
    const books = new Set();
    const paths = new Set();
    const walk = (n) => {
      if (n.book_id) books.add(n.book_id);
      else if (n.path && l.kind === "html") paths.add(n.path);
      n.children.forEach(walk);
    };
    walk(node);
    return {
      lib: l.id,
      books: [...books],
      paths: [...paths],
      rel: l.kind === "folder" || l.kind === "notes" ? node.rel : null,
    };
  }

  const searchItems = (l, node) => [
    { label: "从这里搜索标题…", action: () => onsearchfrom("title", { lib: l.id, label: `${l.title} › ${node.title}`, within: withinOf(l, node) }) },
    { label: "从这里搜索全文…", action: () => onsearchfrom("full", { lib: l.id, label: `${l.title} › ${node.title}`, within: withinOf(l, node) }) },
  ];

  /** Context menu of a node: search from here; notes can also be edited. */
  function nodeMenu(e, l, node, key) {
    if (l.kind === "notes") return noteMenu(e, l, node, key);
    e.preventDefault();
    e.stopPropagation();
    menu = { x: e.clientX, y: e.clientY, items: searchItems(l, node) };
  }

  function noteMenu(e, l, node, key) {
    e.preventDefault();
    e.stopPropagation();
    menu = {
      x: e.clientX,
      y: e.clientY,
      items: [
        ...searchItems(l, node),
        "separator",
        { label: "新建子笔记…", action: () => newNote(l.id, node, key) },
        { label: "重命名…", action: () => renameNote(l.id, node, key) },
        "separator",
        { label: "上移", action: () => shiftNote(l.id, node, key, -1), disabled: !canShift(l.id, node, key, -1) },
        { label: "下移", action: () => shiftNote(l.id, node, key, 1), disabled: !canShift(l.id, node, key, 1) },
        "separator",
        { label: "删除…", action: () => deleteNote(l.id, node, key) },
      ],
    };
  }

  /** Menu of the add button: files (internal libraries only), libraries. */
  export function addMenu(x, y) {
    menu = { x, y, items: addItems() };
  }
  function addItems() {
    return [
      {
        label: "笔记…",
        action: addNoteHere,
        disabled: !libraries.some((l) => l.kind === "notes"), // only internal libraries are writable
      },
      { label: "知识库…", action: onbind },
    ];
  }

  let open = $state({}); // lib id -> expanded
  let nodes = $state({}); // lib id -> tree nodes
  let loading = $state({});
  let errors = $state({});
  let menu = $state(null); // { x, y, items }
  let expanded = $state({}); // lib id -> { node key -> true }
  let current = $state(null); // { lib, key }: the node revealed last
  let panel = $state();

  // a library bound again with another tree: reload it
  let trees = {};
  $effect(() => {
    for (const l of libraries) {
      if (trees[l.id] && trees[l.id] !== l.tree) {
        delete nodes[l.id];
        expanded[l.id] = {};
        if (open[l.id]) load(l.id);
      }
      trees[l.id] = l.tree;
    }
  });

  async function load(id) {
    loading[id] = true;
    errors[id] = "";
    try {
      nodes[id] = await api.tree(id);
    } catch (e) {
      errors[id] = String(e);
    } finally {
      loading[id] = false;
    }
  }

  /** The node at an index path key (`/0/3/5`). */
  function nodeAt(lib, key) {
    let list = nodes[lib] ?? [];
    let n = null;
    for (const i of key.split("/").filter(Boolean)) {
      n = list[Number(i)];
      if (!n) return null;
      list = n.children;
    }
    return n;
  }

  /** Folder libraries list one level at a time. */
  async function loadChildren(lib, n) {
    if (!n?.lazy) return;
    try {
      n.children = await api.folderChildren(lib, n.rel);
      n.lazy = false;
    } catch (e) {
      errors[lib] = String(e);
    }
  }

  async function toggleNode(lib, key) {
    const ex = (expanded[lib] ??= {});
    if (ex[key]) delete ex[key];
    else {
      await loadChildren(lib, nodeAt(lib, key));
      ex[key] = true;
    }
  }

  /** Index path of the node showing a document, or null. CBETA: the leaf of
   * the book (for the parts of a big book, the last part starting at or
   * before the volume); HTML: the article. */
  function findNode(list, { bookId, path }) {
    let exact = null; // an article
    let best = null; // [trail, start]: a CBETA book
    const walk = (ns, trail) => {
      ns.forEach((n, i) => {
        const t = [...trail, i];
        if (path && n.path === path) exact ??= t;
        else if (bookId && n.book_id === bookId) {
          const start = n.start ?? "";
          const fits = !path || !n.start || n.start <= path;
          if (fits && (!best || start > best[1])) best = [t, start];
        }
        walk(n.children, t);
      });
    };
    walk(list, []);
    return exact ?? best?.[0] ?? null;
  }

  const isFolder = (id) => libraries.find((l) => l.id === id)?.kind === "folder";

  /** Index path of a file of a folder library, loading the folders on the way. */
  async function findFolderNode(lib, path) {
    const parts = (path ?? "").split("/");
    let list = nodes[lib] ?? [];
    const trail = [];
    for (let d = 0; d < parts.length; d++) {
      const rel = parts.slice(0, d + 1).join("/");
      const i = list.findIndex((n) => n.rel === rel);
      if (i < 0) return null;
      trail.push(i);
      const n = list[i];
      if (d < parts.length - 1) {
        await loadChildren(lib, n);
        list = n.children;
      }
    }
    return trail;
  }

  /** Open the tree down to the node of a document and select it. */
  export async function reveal({ lib, bookId, path }) {
    if (!libraries.some((l) => l.id === lib)) return false;
    open[lib] = true;
    if (!nodes[lib]) await load(lib);
    const trail = isFolder(lib) ? await findFolderNode(lib, path) : findNode(nodes[lib] ?? [], { bookId, path });
    if (!trail) return false;
    const ex = (expanded[lib] ??= {});
    let key = "";
    for (const i of trail.slice(0, -1)) {
      key += `/${i}`;
      ex[key] = true;
    }
    key += `/${trail[trail.length - 1]}`;
    current = { lib, key };
    await tick();
    panel?.querySelector(`[data-lib="${lib}"] [data-key="${key}"]`)?.scrollIntoView({ block: "center" });
    return true;
  }

  function toggle(l) {
    open[l.id] = !open[l.id];
    if (open[l.id] && !nodes[l.id]) load(l.id);
  }

  function badge(l) {
    if (l.error) return { text: "数据不可用", warn: true };
    const p = progress[l.id];
    if (l.building || p) return { text: p ? `建索引 ${Math.floor((100 * p.done) / Math.max(1, p.total))}%` : "建索引…" };
    if (!l.index_ready) return { text: "未建索引", warn: true };
    if (l.index_data_version && l.data_version && l.index_data_version !== l.data_version)
      return { text: "索引过期", warn: true };
    return null;
  }

  function libMenu(e, l) {
    e.preventDefault();
    e.stopPropagation();
    const building = l.building || !!progress[l.id];
    menu = {
      x: e.clientX,
      y: e.clientY,
      items: [
        { label: open[l.id] ? "收起" : "展开", action: () => toggle(l) },
        ...(l.kind === "notes" ? [{ label: "新建笔记…", action: () => newNote(l.id, null, null) }] : []),
        { label: "从这里搜索标题…", action: () => onsearchfrom("title", { lib: l.id, label: l.title, within: null }) },
        { label: "从这里搜索全文…", action: () => onsearchfrom("full", { lib: l.id, label: l.title, within: null }) },
        "separator",
        { label: l.index_ready ? "重建索引" : "建立索引", action: () => onindex(l.id), disabled: building || !!l.error },
        ...(l.kind === "folder" ? [] : [{ label: "从同一数据包绑定其他知识库…", action: () => onrebind(l) }]),
        "separator",
        { label: l.kind === "notes" ? "删除知识库…" : "解除绑定", action: () => onunbind(l), disabled: building },
      ],
    };
  }

  function panelMenu(e) {
    e.preventDefault();
    menu = { x: e.clientX, y: e.clientY, items: addItems() };
  }
</script>

<div class="panel" bind:this={panel} role="tree" aria-label="知识库" oncontextmenu={panelMenu}>
  {#each libraries as l (l.id)}
    {@const b = badge(l)}
    <div class="lib" data-lib={l.id}>
      <div class="root" role="treeitem" aria-expanded={!!open[l.id]} aria-selected="false" tabindex="0"
        oncontextmenu={(e) => libMenu(e, l)}
        onkeydown={(e) => (e.key === "Enter" || e.key === " ") && (e.preventDefault(), toggle(l))}>
        <button class="twisty" onclick={() => toggle(l)} tabindex="-1" aria-hidden="true">
          <Icon name={open[l.id] ? "chevron-down" : "chevron-right"} size={14} />
        </button>
        <span class="libicon"><Icon name="library" size={18} /></span>
        <button class="title" onclick={() => toggle(l)} tabindex="-1" title={`${l.title}\n${l.source}`}>{l.title}</button>
        {#if b}<span class="badge" class:warn={b.warn}>{b.text}</span>{/if}
      </div>
      {#if open[l.id]}
        {#if loading[l.id]}
          <p class="muted note">载入中…</p>
        {:else if errors[l.id]}
          <p class="error note">{errors[l.id]}</p>
        {:else if nodes[l.id]}
          <Tree
            nodes={nodes[l.id]}
            depth={1}
            expanded={expanded[l.id] ?? {}}
            current={current?.lib === l.id ? current.key : null}
            ontoggle={(key) => toggleNode(l.id, key)}
            oncontext={(node, key, e) => nodeMenu(e, l, node, key)}
            onopen={(node, key) => {
              selected = { lib: l.id, key };
              onopen(l, node);
            }} />
        {/if}
      {/if}
    </div>
  {:else}
    <div class="empty">
      <p>还没有绑定知识库。</p>
      <button class="primary" onclick={onbind}>绑定知识库…</button>
      <p class="muted small">也可以在这里点右键绑定。</p>
    </div>
  {/each}
</div>

{#if noteDlg}
  <NoteDialog {...noteDlg} onok={(pos, title) => addNote(noteDlg.defaultLib, pos, title)} onclose={() => (noteDlg = null)} />
{/if}

{#if nameDlg}
  <NameDialog {...nameDlg} onclose={() => (nameDlg = null)} />
{/if}

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} items={menu.items} onclose={() => (menu = null)} />
{/if}

<style>
  .panel { min-height: 100%; padding-bottom: 40px; }
  .lib { margin-bottom: 4px; }
  .root {
    display: flex; align-items: center; gap: 2px; min-height: 30px; border-radius: 6px; padding-right: 2px;
  }
  .root:hover, .root:focus-visible { background: var(--accent-soft); outline: none; }
  .twisty {
    width: 22px; height: 22px; flex: none; border: none; background: none; padding: 0; color: var(--muted);
    display: inline-flex; align-items: center; justify-content: center; border-radius: 4px;
  }
  .twisty:hover { background: var(--accent-soft); color: var(--accent); }
  .libicon { display: inline-flex; color: var(--accent); }
  .title {
    flex: 1; min-width: 0; border: none; background: none; text-align: left; padding: 2px 4px;
    font-weight: 700; color: var(--accent); white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  .badge { font-size: 11px; color: var(--muted); white-space: nowrap; }
  .badge.warn { color: var(--hl); }
  .note { font-size: 13px; padding-left: 24px; }
  .error { color: var(--hl); }
  .empty { padding: 24px 12px; text-align: center; }
  .small { font-size: 13px; }
</style>
