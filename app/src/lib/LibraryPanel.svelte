<script>
  // The tree panel: one root per bound knowledge library, its tree loaded
  // when first opened. Right-click a library for its actions, or the empty
  // panel to bind a library.
  import { tick } from "svelte";
  import * as api from "./api.js";
  import Tree from "./Tree.svelte";
  import ContextMenu from "./ContextMenu.svelte";
  import NameDialog from "./NameDialog.svelte";
  import Icon from "./Icon.svelte";
  import * as workspace from "./workspace.svelte.js";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { fullPath } from "./paths.js";
  import { monthPath, localDate } from "./diary.js";

  let { libraries, progress, onopen, onbind, onrebind, onunbind, onindex, onsearchfrom, ontag, oncopy, onexport, onimport, onlibs = null, onerror = null } = $props();

  // what was ever opened, per library: shown in green (not "finished", only "opened")
  let seen = $state({}); // lib id -> { books: Set, paths: Set }
  async function loadSeen(id) {
    try {
      const o = await api.openedSet(id);
      seen[id] = { books: new Set(o.books), paths: new Set(o.paths) };
    } catch {}
  }
  $effect(() => {
    const _n = workspace.ws.opened;
    for (const id of Object.keys(nodes)) loadSeen(id);
  });

  // several nodes picked with Ctrl / Shift + click (one library at a time)
  let multi = $state({ lib: null, keys: new Set(), anchor: null });
  const clearPicked = () => {
    if (multi.keys.size) multi = { lib: null, keys: new Set(), anchor: null };
  };

  function pickNode(l, node, key, e) {
    const same = multi.lib === l.id;
    const keys = new Set(same ? multi.keys : []);
    if (!keys.size && current?.lib === l.id) keys.add(current.key); // the node being read joins the picked ones
    const anchor = same && multi.anchor ? multi.anchor : current?.lib === l.id ? current.key : null;
    if (e.shiftKey && anchor) {
      const order = [...panel.querySelectorAll(`[data-lib="${l.id}"] [data-key]`)].map((el) => el.dataset.key);
      const a = order.indexOf(anchor);
      const b = order.indexOf(key);
      if (a >= 0 && b >= 0) {
        if (!(e.ctrlKey || e.metaKey)) keys.clear();
        for (let i = Math.min(a, b); i <= Math.max(a, b); i++) keys.add(order[i]);
        multi = { lib: l.id, keys, anchor };
        return;
      }
    }
    if (keys.has(key)) keys.delete(key);
    else keys.add(key);
    multi = { lib: l.id, keys, anchor: key };
  }

  /** The picked nodes of a library. */
  const pickedNodes = (l) => [...multi.keys].map((k) => ({ key: k, node: nodeAt(l.id, k) })).filter((x) => x.node);

  /** One search range for several nodes. */
  function withinMany(l, list) {
    const books = new Set();
    const paths = new Set();
    const rels = [];
    for (const { node } of list) {
      const w = withinOf(l, node);
      w.books.forEach((b) => books.add(b));
      w.paths.forEach((p) => paths.add(p));
      if (w.rel) rels.push(w.rel);
    }
    return { lib: l.id, books: [...books], paths: [...paths], rels };
  }

  /** The name an export of several nodes is called: their common parent, else the library. */
  function parentName(l, list) {
    const parents = new Set(list.map((x) => x.key.slice(0, x.key.lastIndexOf("/"))));
    if (parents.size !== 1) return l.title;
    const p = [...parents][0];
    return p ? titleChain(l, p).at(-1) : l.title;
  }

  function multiMenu(e, l) {
    const list = pickedNodes(l);
    const label = `${l.title} › 已选 ${list.length} 项`;
    menu = {
      x: e.clientX,
      y: e.clientY,
      items: [
        { label: `已选 ${list.length} 项`, action: () => {}, disabled: true },
        "separator",
        { label: "复制目录树路径", action: () => oncopy(list.map(({ key }) => titleChain(l, key).join(" › ")).join("\n")) },
        "separator",
        { label: "从所选节点搜索标题…", action: () => onsearchfrom("title", { lib: l.id, label, within: withinMany(l, list) }) },
        { label: "从所选节点搜索全文…", action: () => onsearchfrom("full", { lib: l.id, label, within: withinMany(l, list) }) },
        "separator",
        {
          label: "导出所选…",
          action: () =>
            onexport(l, { whole: false, keys: list.map((x) => x.key), rels: list.map((x) => x.node.rel).filter(Boolean) }, `已选 ${list.length} 项`, parentName(l, list)),
        },
        "separator",
        { label: "取消选择", action: clearPicked },
      ],
    };
  }

  let nameDlg = $state(null); // { title, label, value, okLabel, onok }
  let selected = $state(null); // { lib, key }: the last note opened from the tree

  const isNotes = (id) => libraries.find((l) => l.id === id)?.kind === "notes";
  const parentKey = (key) => key.slice(0, key.lastIndexOf("/"));

  /** Reload one level of a library (`key` null: its top), keeping what was
   * loaded below the notes that are still there. */
  /** Read a node (or the top of a library) again, after notes were added below it. */
  export async function reloadNode(lib, key) {
    open[lib] = true;
    await refresh(lib, key);
  }

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

  /** Add a note, a mind map or a whiteboard (`kind`) below the node at `key`
   * (`null`: at the top of the library `lib`), with a name of its own
   * (`笔记 1`, …) that can be changed any time, and open it. */
  async function createNote(lib, key, kind = "md") {
    try {
      const r = await api.createNoteAt(lib, key ? pathOf(lib, key) : "", null, kind);
      onlibs?.(); // a first note makes its library
      await tick();
      await showNew(r);
      await workspace.openVolume({ lib: r.lib, path: r.id });
    } catch (e) {
      if (lib) errors[lib] = String(e);
      else onerror?.(String(e));
    }
  }

  /** Show a new note in the tree (`select`: and select it). */
  export async function showNew(r, select = true) {
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
    if (select && idx >= 0) {
      current = { lib: r.lib, key: `${key}/${idx}` };
      selected = current;
      await tick();
      panel?.querySelector(`[data-lib="${r.lib}"] [data-key="${current?.key}"]`)?.scrollIntoView({ block: "center" });
    }
  }

  /** From the add menu: below the selected note, else in the library of the last one. */
  function addNoteHere(kind) {
    const sel = selected && isNotes(selected.lib) ? selected : null;
    createNote(sel?.lib ?? null, sel?.key ?? null, kind);
  }

  /** Today's diary: shown in the tree and opened. */
  export async function openDiary() {
    try {
      const r = await api.diaryToday(localDate());
      onlibs?.();
      await tick();
      await showNew(r.note);
      await workspace.openVolume({ lib: r.note.lib, path: r.note.id });
    } catch (e) {
      onerror?.(String(e));
    }
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

  /** The titles from the library down to a node (`key` is its index path). */
  function titleChain(l, key) {
    let list = nodes[l.id];
    const out = [l.title];
    for (const i of key.split("/").filter(Boolean).map(Number)) {
      const n = list?.[i];
      if (!n) break;
      out.push(n.title);
      list = n.children;
    }
    return out;
  }

  /** The original file: a file path for folders, `<package>!/<path>` for data packages. */
  function filePath(l, node) {
    const p = node.path ?? node.rel ?? node.start;
    return p ? fullPath(l, p) : l.source;
  }

  /** The titles from the library down to a document, as text (null when it is not in the tree). */
  export async function treePath({ lib, bookId, path }) {
    const l = libraries.find((x) => x.id === lib);
    if (!l) return null;
    try {
      if (!nodes[lib]) await load(lib);
      const trail = await trailOf(lib, bookId, path);
      if (!trail) return null;
      let list = nodes[lib];
      const out = [l.title];
      for (const i of trail) {
        out.push(list[i].title);
        list = list[i].children;
      }
      return out.join(" › ");
    } catch {
      return null;
    }
  }

  /** Menu sections (lists of items) joined by separators; empty ones are left out. */
  const sections = (...groups) => groups.filter((g) => g.length).flatMap((g, i) => (i ? ["separator", ...g] : g));

  /** Items of a node's menu, grouped: new, edit, copy, search, tags, import / export. */
  function nodeItems(l, node, key) {
    const notes = l.kind === "notes";
    const label = `${l.title} › ${node.title}`;
    return sections(
      notes
        ? [
            { label: "新建笔记", action: () => createNote(l.id, key, "md") },
            { label: "新建脑图", action: () => createNote(l.id, key, "mindmap") },
            { label: "新建白板", action: () => createNote(l.id, key, "whiteboard") },
          ]
        : [],
      notes
        ? [
            { label: "重命名…（F2）", action: () => renameNote(l.id, node, key) },
            { label: "上移", action: () => shiftNote(l.id, node, key, -1), disabled: !canShift(l.id, node, key, -1) },
            { label: "下移", action: () => shiftNote(l.id, node, key, 1), disabled: !canShift(l.id, node, key, 1) },
            { label: "删除…", action: () => deleteNote(l.id, node, key) },
          ]
        : [],
      [
        ...(notes ? [] : [{ label: "复制原文件路径", action: () => oncopy(filePath(l, node)) }]),
        { label: "复制目录树路径", action: () => oncopy(titleChain(l, key).join(" › ")) },
      ],
      [
        { label: "从这里搜索标题…", action: () => onsearchfrom("title", { lib: l.id, label, within: withinOf(l, node) }) },
        { label: "从这里搜索全文…", action: () => onsearchfrom("full", { lib: l.id, label, within: withinOf(l, node) }) },
      ],
      node.book_id || node.path || node.rel ? [{ label: "标签…", action: () => ontag(l, node) }] : [],
      [
        ...(notes ? [{ label: "导入到这里（作为子节点）…", action: () => onimport({ lib: l.id, parent: node.rel, key }) }] : []),
        { label: "导出此节点…", action: () => onexport(l, { whole: false, keys: [key], rels: node.rel ? [node.rel] : [] }, node.title, node.title) },
      ],
    );
  }

  /** Context menu of a node. */
  function nodeMenu(e, l, node, key) {
    e.preventDefault();
    e.stopPropagation();
    if (multi.lib === l.id && multi.keys.size > 1 && multi.keys.has(key)) return multiMenu(e, l);
    openMenu({ x: e.clientX, y: e.clientY, items: nodeItems(l, node, key) }, { lib: l.id, key });
  }

  /** Menu of the add button: files (internal libraries only), libraries. */
  export function addMenu(x, y) {
    menu = { x, y, items: addItems() };
  }
  function addItems() {
    return [
      { label: "新建笔记", action: () => addNoteHere("md") },
      { label: "新建脑图", action: () => addNoteHere("mindmap") },
      { label: "新建白板", action: () => addNoteHere("whiteboard") },
      { label: "今日日记（Ctrl+J）", action: openDiary },
      "separator",
      { label: "新建知识库…", action: onbind },
      { label: "导入知识库（导出包）…", action: () => onimport(null) },
    ];
  }

  let open = $state({}); // lib id -> expanded
  let nodes = $state({}); // lib id -> tree nodes
  let loading = $state({});
  let errors = $state({});
  let menu = $state(null); // { x, y, items }
  let menuMark = $state(null); // { lib, key }: the node (key null: the library) whose menu is open
  const openMenu = (m, mark = null) => ((menuMark = mark), (menu = m));

  /** Index path key of the node showing the document of the active reading tab (if it is loaded). */
  const activeKey = $derived.by(() => {
    const c = workspace.ws.current;
    if (!c || !nodes[c.lib]) return null;
    const trail = findNode(nodes[c.lib], { bookId: c.bookId, path: c.path });
    return trail ? { lib: c.lib, key: "/" + trail.join("/") } : null;
  });
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

  const loadings = {}; // lib id -> the running load
  function load(id) {
    return (loadings[id] ??= doLoad(id).finally(() => delete loadings[id]));
  }
  async function doLoad(id) {
    loading[id] = true;
    errors[id] = "";
    try {
      nodes[id] = await api.tree(id);
      loadSeen(id);
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
      // opening the current month of the diary makes sure that today's entry is there
      if (libraries.find((l) => l.id === lib)?.title === monthPath().split("/")[0] && pathOf(lib, key) === monthPath()) {
        try {
          const r = await api.diaryToday(localDate());
          await refresh(lib, key);
        } catch {}
      }
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

  /** Index path of a note of an internal library: its chain of ids, loading the levels on the way. */
  async function findNoteNode(lib, id) {
    let chain;
    try {
      chain = await api.noteChain(lib, id);
    } catch {
      return null; // deleted
    }
    let list = nodes[lib] ?? [];
    const trail = [];
    for (let d = 0; d < chain.length; d++) {
      const i = list.findIndex((n) => n.rel === chain[d]);
      if (i < 0) return null;
      trail.push(i);
      if (d < chain.length - 1) {
        await loadChildren(lib, list[i]);
        list = list[i].children;
      }
    }
    return trail;
  }

  /** Index path of a document in the tree (loading folders on the way). */
  const trailOf = (lib, bookId, path) =>
    isFolder(lib) ? findFolderNode(lib, path) : isNotes(lib) ? findNoteNode(lib, path) : findNode(nodes[lib] ?? [], { bookId, path });

  /** Open the tree down to the node of a document and select it. */
  export async function reveal({ lib, bookId, path }) {
    if (!libraries.some((l) => l.id === lib)) return false;
    open[lib] = true;
    if (!nodes[lib]) await load(lib);
    const trail = await trailOf(lib, bookId, path);
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
    const notes = l.kind === "notes";
    menuMark = { lib: l.id, key: null };
    menu = {
      x: e.clientX,
      y: e.clientY,
      items: sections(
        [{ label: open[l.id] ? "收起" : "展开", action: () => toggle(l) }],
        notes
          ? [
              { label: "新建笔记", action: () => createNote(l.id, null, "md") },
              { label: "新建脑图", action: () => createNote(l.id, null, "mindmap") },
              { label: "新建白板", action: () => createNote(l.id, null, "whiteboard") },
            ]
          : [],
        [{ label: "重命名…（F2）", action: () => renameLibrary(l) }],
        [
          ...(notes ? [] : [{ label: "复制原文件路径", action: () => oncopy(l.source) }]),
          { label: "复制目录树路径", action: () => oncopy(l.title) },
        ],
        [
          { label: "从这里搜索标题…", action: () => onsearchfrom("title", { lib: l.id, label: l.title, within: null }) },
          { label: "从这里搜索全文…", action: () => onsearchfrom("full", { lib: l.id, label: l.title, within: null }) },
          { label: l.index_ready ? "重建索引" : "建立索引", action: () => onindex(l.id), disabled: building || !!l.error },
        ],
        [
          ...(notes ? [{ label: "导入到此知识库…", action: () => onimport({ lib: l.id, parent: null, key: null }) }] : []),
          { label: "导出知识库…", action: () => onexport(l, { whole: true, keys: [], rels: [] }, l.title, l.title) },
          ...(l.kind === "folder" ? [] : [{ label: "绑定知识库…", action: () => onrebind(l) }]),
          { label: notes ? "删除知识库…" : "解除绑定", action: () => onunbind(l), disabled: building },
        ],
      ),
    };
  }

  /** Another name for a library; a bound one is only shown under it, its data stays as it is. */
  function renameLibrary(l) {
    nameDlg = {
      title: "重命名知识库",
      label: "名称",
      value: l.title,
      okLabel: "确定",
      onok: guard(l.id, async (title) => {
        await api.renameLibrary(l.id, title);
        await onlibs?.();
      }),
    };
  }

  /** F2 renames what has the focus in the tree (a library, or a note), else the last note opened from the tree. */
  function f2(e) {
    if (e.key !== "F2" || e.ctrlKey || e.altKey || e.shiftKey || nameDlg) return;
    const t = e.target;
    if (t?.closest?.("input, textarea, select, [contenteditable], .ProseMirror, .excalidraw")) return;
    const lib = libraries.find((x) => x.id === t?.closest?.("[data-lib]")?.dataset.lib);
    const key = t?.closest?.("[data-key]")?.dataset.key;
    if (lib && !key) {
      e.preventDefault();
      return renameLibrary(lib);
    }
    const at = lib && key ? { lib: lib.id, key } : selected && isNotes(selected.lib) ? selected : null;
    const node = at ? nodeAt(at.lib, at.key) : null;
    if (node && isNotes(at.lib)) {
      e.preventDefault();
      renameNote(at.lib, node, at.key);
    }
  }

  function panelMenu(e) {
    e.preventDefault();
    menu = { x: e.clientX, y: e.clientY, items: addItems() };
  }
</script>

<svelte:window onkeydown={f2} />

<div class="panel" bind:this={panel} role="tree" aria-label="知识库" oncontextmenu={panelMenu}>
  {#each libraries as l (l.id)}
    {@const b = badge(l)}
    <div class="lib" data-lib={l.id}>
      <div class="root" class:marked={menuMark?.lib === l.id && menuMark.key === null} role="treeitem" aria-expanded={!!open[l.id]} aria-selected="false" tabindex="0"
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
            active={activeKey?.lib === l.id ? activeKey.key : null}
            marked={menuMark?.lib === l.id ? menuMark.key : null}
            seen={seen[l.id] ?? null}
            picked={multi.lib === l.id ? multi.keys : null}
            onselect={(node, key, e) => pickNode(l, node, key, e)}
            onclear={clearPicked}
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

{#if nameDlg}
  <NameDialog {...nameDlg} onclose={() => (nameDlg = null)} />
{/if}

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} items={menu.items} onclose={() => ((menu = null), (menuMark = null))} />
{/if}

<style>
  .panel { min-height: 100%; padding-bottom: 40px; }
  .lib { margin-bottom: 4px; }
  .root {
    display: flex; align-items: center; gap: 2px; min-height: 30px; border-radius: 6px; padding-right: 2px;
  }
  .root:hover, .root:focus-visible { background: var(--accent-soft); outline: none; }
  .root.marked { background: color-mix(in srgb, var(--accent) 14%, transparent); }
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
