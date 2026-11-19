<script>
  // The tree panel: one root per bound knowledge library, its tree loaded
  // when first opened. Right-click a library for its actions, or the empty
  // panel to bind a library.
  import { tick, untrack } from "svelte";
  import * as api from "./api.js";
  import Tree from "./Tree.svelte";
  import ContextMenu from "./ContextMenu.svelte";
  import NameDialog from "./NameDialog.svelte";
  import MoveDialog from "./MoveDialog.svelte";
  import TrashDialog from "./TrashDialog.svelte";
  import NotesTrashDialog from "./NotesTrashDialog.svelte";
  import Icon from "./Icon.svelte";
  import * as workspace from "./workspace.svelte.js";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { fullPath } from "./paths.js";
  import { DIARY_LIBRARY_ID, monthNodeId, monthPath, localDate } from "./diary.js";
  import { startScopeDrag } from "./scopeDrag.js";
  import { installTextDrag } from "./textDrag.js";
  import { confirmWritable, isEditableName } from "./writable.js";
  import { isAiReady, toBackendAiConfig } from "./settings.svelte.js";
  import { matchesShortcut } from "./shortcuts.js";

  let { libraries, progress, onopen, onbind, onrebind, onunbind, onindex, onsearchfrom, ontag, onprops, oncopy, onexport, onimport, onlibs = null, onerror = null } = $props();

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
        { label: "加入搜索范围", action: () => list.forEach(({ key, node }) => onsearchfrom("scope-add", { lib: l.id, label: titleChain(l, key).join(" › "), within: withinOf(l, node) })) },
        { label: "加入每日一句来源", action: () => list.forEach(({ key, node }) => onsearchfrom("daily-add", { lib: l.id, label: titleChain(l, key).join(" › "), within: withinOf(l, node) })) },
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
  let moveDlg = $state(null); // { from, dirs, onok }
  let trashDlg = $state(null); // { lib, title }: the trash of a read-write local folder
  let notesTrashDlg = $state(null); // { lib, title }: deleted notes of an internal library
  let selected = $state(null); // { lib, key }: the last note opened from the tree

  const isNotes = (id) => libraries.find((l) => l.id === id)?.kind === "notes";
  /** Notes can be edited: internal libraries, and external folders in read-write mode. */
  const isEditable = (id) => {
    const l = libraries.find((x) => x.id === id);
    return l?.kind === "notes" || (l?.kind === "folder" && !!l.writable);
  };
  const isRwFolder = (id) => {
    const l = libraries.find((x) => x.id === id);
    return l?.kind === "folder" && !!l.writable;
  };
  const parentKey = (key) => key.slice(0, key.lastIndexOf("/"));

  /** The icon and the name of the kind of a library: data package, local folder (read-only / read-write), internal. */
  const kindIcon = (l) => (l.kind === "notes" ? "database" : l.kind === "folder" ? (l.writable ? "folder-edit" : "folder-lock") : "archive");
  const kindLabel = (l) =>
    l.kind === "notes" ? "内部知识库（数据库，可编辑）" : l.kind === "folder" ? (l.writable ? "本地文件夹（读写）" : "本地文件夹（只读）") : "数据包（只读）";

  /** The folder a new file or folder goes into for the node at `key`: the node itself when it is a folder, else its folder. */
  function dirOf(lib, key) {
    const n = key ? nodeAt(lib, key) : null;
    return !n ? "" : n.path == null ? n.rel : n.rel.split("/").slice(0, -1).join("/");
  }

  /** Make a folder in a read-write local folder (the library's top when `key` is null). */
  function newFolder(lib, key) {
    nameDlg = {
      title: "新建文件夹",
      label: "文件夹名称",
      value: "",
      okLabel: "新建",
      hint: `${nameRules}\n可以用 / 或 \\ 分隔，一次建立多级文件夹（例如 资料/读书/2026）。`,
      validate: checkPath,
      onok: guard(lib, async (name) => {
        const r = await api.createFolderDir(lib, dirOf(lib, key), name);
        await showNew(r);
      }),
    };
  }

  /** The rules for local file names, shown under the box that asks for one. */
  let nameRules = "";
  api.fileNameRules().then((t) => (nameRules = t)).catch(() => {});
  const checkPath = async (t) => {
    try {
      await api.checkFilePath(t);
      return "";
    } catch (e) {
      return String(e);
    }
  };
  const checkName = async (t) => {
    try {
      await api.checkFileName(t);
      return "";
    } catch (e) {
      return String(e);
    }
  };

  /** Switch an external folder between read-only and read-write mode. Going to
   * read-write needs the user to accept the rules first. */
  async function setWritable(l, on) {
    try {
      if (on && !(await confirmWritable(l.title))) return;
      // the tree stays as it is (the mode is only a state of the library); the open readers differ, so they are opened again
      await workspace.reopenLibrary(l.id, async () => {
        await api.setFolderWritable(l.id, on);
        await onlibs?.();
      });
    } catch (e) {
      onerror?.(String(e));
    }
  }

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
        expansionOf(lib)[key] = true;
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
      throw e; // the dialog that asked for it shows the reason
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
      let r;
      if (lib && isRwFolder(lib)) {
        // a file in a folder of the library: inside the folder at `key`, next to the file at `key`
        r = await api.createFolderFile(lib, dirOf(lib, key), kind);
      } else {
        r = await api.createNoteAt(lib, key ? pathOf(lib, key) : "", null, kind);
      }
      onlibs?.(); // a first note makes its library
      await tick();
      await showNew(r);
      await workspace.openVolume({ lib: r.lib, path: r.id });
    } catch (e) {
      onerror?.(String(e)); // a failed action must not hide the tree
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
    if (idx >= 0) {
      // a restored note can take the place of an older node at the same index:
      // start folded, so its children are read only when the user opens it
      const nodeKey = `${key}/${idx}`;
      const ex = expansionOf(r.lib);
      for (const k of Object.keys(ex)) if (k === nodeKey || k.startsWith(`${nodeKey}/`)) delete ex[k];
      if (select) {
        current = { lib: r.lib, key: nodeKey };
        selected = current;
        await tick();
        panel?.querySelector(`[data-lib="${r.lib}"] [data-key="${current?.key}"]`)?.scrollIntoView({ block: "center" });
      }
    }
  }

  /** A new note (Ctrl+N): where the add menu puts it. */
  export function newNote(kind = "md") {
    addNoteHere(kind);
  }

  /** From the add menu: below the selected note, else in the library of the last one. */
  function addNoteHere(kind) {
    const sel = selected && isEditable(selected.lib) ? selected : null;
    createNote(sel?.lib ?? null, sel?.key ?? null, kind);
  }

  export const newMindMap = () => addNoteHere("mindmap");
  export const newWhiteboard = () => addNoteHere("whiteboard");

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
    const file = isRwFolder(lib); // a file or folder name: strict rules, and the path changes with it
    const dir = file && node.path == null;
    nameDlg = {
      title: dir ? `重命名文件夹：${node.title}` : `重命名：${node.title}`,
      nodePath: pathOf(lib, key),
      label: dir ? "文件夹名称" : file ? "文件名（不含后缀）" : "标题",
      value: node.title,
      okLabel: "确定",
      hint: file ? nameRules : "",
      validate: file ? checkName : null,
      aiSuggest: !dir && isAiReady()
        ? async () => {
            const doc = await api.aiDocText(lib, null, node.path ?? node.rel);
            const body = (doc?.text || "").trim();
            if (!body) throw new Error("笔记内容为空，请先输入内容后再使用 AI 自动命名");
            let suggested = await api.aiSummarizeNoteTitle(toBackendAiConfig(), body);
            if (file) suggested = suggested.replace(/[\\/:*?"<>|]/g, "").trim();
            return suggested;
          }
        : null,
      onok: guard(lib, async (title) => {
        if (!file) {
          await api.renameNote(lib, node.rel, title);
          workspace.retitle(lib, node.rel, title);
        } else {
          await relocate(lib, node, () => api.renameNote(lib, node.rel, title));
        }
        await refresh(lib, parentKey(key) || null);
      }),
    };
  }

  /** Change the path of a file or folder of a read-write folder (`change` resolves to the new path): the
   * tabs of what is inside are closed first (their editors save), and the active one opens again under the new path. */
  async function relocate(lib, node, change) {
    const { active } = await workspace.closeUnder(lib, node.rel);
    let rel;
    try {
      rel = await change();
    } catch (e) {
      if (active) workspace.openVolume({ lib, path: active });
      throw e;
    }
    if (active) workspace.openVolume({ lib, path: rel + active.slice(node.rel.length) });
    return rel;
  }

  const chainOf = (rel) => rel.split("/").map((_, i, a) => a.slice(0, i + 1).join("/"));

  /** Pick a folder and move a file or folder of a read-write folder into it. */
  async function moveNode(lib, node, key) {
    try {
      moveDlg = {
        title: `移动到：${node.title}`,
        nodePath: pathOf(lib, key),
        from: node.rel,
        dirs: await api.folderDirs(lib),
        onok: guard(lib, async (parent) => {
          const rel = await relocate(lib, node, () => api.moveFolderNode(lib, node.rel, parent));
          await refresh(lib, parentKey(key) || null);
          await showNew({ lib, id: rel, chain: chainOf(rel) });
        }),
      };
    } catch (e) {
      onerror?.(String(e));
    }
  }

  /** Pick a target note (or library root) and move an internal note into it. */
  async function moveNoteNode(lib, node, key) {
    try {
      moveDlg = {
        title: `移动到：${node.title}`,
        nodePath: pathOf(lib, key),
        dirs: await api.noteMoveTargets(lib, node.rel),
        onok: guard(lib, async (targetParent) => {
          await api.moveNote(lib, node.rel, targetParent || null, 999999);
          await reconcile(lib, null);
          const trail = targetParent ? await findNoteNode(lib, targetParent) : [];
          if (trail) {
            let k = "";
            for (const i of trail) {
              k += `/${i}`;
              expansionOf(lib)[k] = true;
            }
            if (k) await refresh(lib, k);
          }
        }),
      };
    } catch (e) {
      onerror?.(String(e));
    }
  }

  async function deleteNote(lib, node, key) {
    const file = isRwFolder(lib);
    const dir = file && node.path == null;
    const more = file ? "" : node.lazy || node.children.length ? "\n它下面的子笔记会一起移到回收站。" : "";
    const ok = await ask(
      dir
        ? `删除文件夹「${node.rel}」？\n里面的所有文件和子文件夹会一起移到回收站（.smart-library/trash），不会彻底删除。\n可以在知识库右键菜单的「回收站…」里恢复。`
        : file
        ? `删除文件「${node.rel}」？\n它会移到回收站（.smart-library/trash），不会彻底删除。\n可以在知识库右键菜单的「回收站…」里恢复。`
        : `删除笔记「${node.title}」？${more}\n内容和属性（含标签、别名）会一起保存，可以在知识库右键菜单的「回收站…」里恢复。`,
      {
        title: dir ? "删除文件夹" : file ? "删除文件" : "删除笔记",
        kind: "warning",
        okLabel: "删除",
        cancelLabel: "取消",
      },
    );
    if (!ok) return;
    try {
      if (dir) {
        await workspace.closeUnder(lib, node.rel); // the editors of what is inside save first
        await api.deleteFolderDir(lib, node.rel);
      } else {
        await api.deleteNote(lib, node.rel);
        workspace.closeDocument(lib, node.rel);
      }
      if (selected?.lib === lib && selected.key.startsWith(key)) selected = null;
      await refresh(lib, parentKey(key) || null);
    } catch (e) {
      onerror?.(String(e));
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
      onerror?.(String(e));
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

  /** A node (or a whole library) as an item of a search range list. */
  function scopeOf(l, node, key) {
    if (!node) return { lib: l.id, label: l.title, books: [], paths: [], rels: [], whole: true };
    const w = withinOf(l, node);
    return { lib: l.id, label: titleChain(l, key).join(" › "), books: w.books, paths: w.paths, rels: w.rel ? [w.rel] : [], whole: false };
  }

  /** Dragging a node (or the picked nodes, when it is one of them) into a search range list. */
  function dragNode(l, node, key, e) {
    // in a read-write folder the nodes can also be dropped on a folder of the tree to move them
    const move =
      node && isNotes(l.id)
        ? { reorder: true, check: (row, zone) => noteVerdict(l, node, key, row, zone), onDrop: (row, zone) => noteDrop(l, node, key, row, zone) }
        : node && isRwFolder(l.id) && (node.path == null || isEditableName(node.rel))
          ? { check: (row) => moveVerdict(l, node, key, row), onDrop: (row) => dropMove(l, node, key, row) }
          : null;
    startScopeDrag(
      e,
      () => {
        if (!node) return [scopeOf(l, null)];
        const list = multi.lib === l.id && multi.keys.size > 1 && multi.keys.has(key) ? pickedNodes(l) : [{ key, node }];
        return list.map((x) => scopeOf(l, x.node, x.key));
      },
      move,
    );
  }

  const parentOf = (r) => r.split("/").slice(0, -1).join("/");
  const lastOf = (r) => r.split("/").pop();

  /** The notes (index path + node) that dropping on `row` would really move: the dragged one or the picked ones,
   * not what is inside another moved note, not what is already a child of the target, not a note into itself. */
  function noteMoveList(l, node, key, row, zone = "in") {
    const rowKey = row.dataset.key ?? ""; // "" : the root of the library (top level)
    const group = multi.lib === l.id && multi.keys.size > 1 && multi.keys.has(key) ? pickedNodes(l) : [{ key, node }];
    const keys = group.map((x) => x.key);
    const alone = (x) => !keys.some((o) => o !== x.key && x.key.startsWith(o + "/"));
    // before / after a row: the notes join the level of that row; into: they become its last children
    const beside = zone !== "in" && rowKey !== "";
    const tkey = beside ? parentKey(rowKey) : rowKey;
    const trel = beside ? (tkey ? (nodeAt(l.id, tkey)?.rel ?? null) : null) : row.dataset.rel || null;
    const todo = group.filter((x) => alone(x) && tkey !== x.key && !tkey.startsWith(x.key + "/") && (beside ? rowKey !== x.key && !rowKey.startsWith(x.key + "/") : parentKey(x.key) !== tkey));
    let index = 1e9;
    if (beside) {
      const moved = new Set(todo.map((x) => x.node.rel));
      const siblings = (tkey ? nodeAt(l.id, tkey)?.children : nodes[l.id]) ?? [];
      const rest = siblings.filter((n) => !moved.has(n.rel));
      const at = rest.findIndex((n) => n.rel === row.dataset.rel);
      index = at < 0 ? 1e9 : at + (zone === "after" ? 1 : 0);
      // nothing moves when the notes stay where they are in the same order
      if (todo.length && todo.every((x) => parentKey(x.key) === tkey)) {
        const order = siblings.map((n) => n.rel);
        const want = [...rest.map((n) => n.rel)];
        want.splice(index, 0, ...todo.map((x) => x.node.rel));
        if (want.every((r, i) => r === order[i])) return { tkey, trel, todo: [], index };
      }
    }
    return { tkey, trel, todo, index };
  }

  /** While dragging a note: into another note, to the top (the library's row), or before / after a row. */
  function noteVerdict(l, node, key, row, zone) {
    const rowLib = row.closest("[data-lib]")?.dataset.lib;
    if (!rowLib) return null;
    if (rowLib !== l.id) return "no";
    return noteMoveList(l, node, key, row, zone).todo.length ? "ok" : "no";
  }

  /** Dropped: the notes go where the verdict said. */
  async function noteDrop(l, node, key, row, zone) {
    const { trel, todo, index } = noteMoveList(l, node, key, row, zone);
    if (!todo.length) return;
    const fails = [];
    let at = index;
    for (const x of todo) {
      try {
        await api.moveNote(l.id, x.node.rel, trel, at);
        at += at >= 1e9 ? 0 : 1;
      } catch (e) {
        fails.push(`${x.node.title}：${e}`);
      }
    }
    clearPicked();
    selected = null;
    await reconcile(l.id, null);
    // show where they went
    const trail = trel ? await findNoteNode(l.id, trel) : [];
    if (trail) {
      let k = "";
      for (const i of trail) {
        k += `/${i}`;
        expansionOf(l.id)[k] = true;
      }
      if (k) await refresh(l.id, k); // the target may have been a leaf: read what it holds now
    }
    if (fails.length) onerror?.(fails.join("\n"));
  }

  /** The folder a row stands for when something is dropped on it: the folder itself, or the folder of a file. */
  const dropTarget = (row) => {
    const rel = row.dataset.rel ?? "";
    return row.dataset.isdir === "1" ? rel : parentOf(rel);
  };

  /** The nodes of a folder as far as they are loaded (null: not read yet). `row`: a row of the tree or the library's root row. */
  function loadedChildren(lib, row) {
    const key = row.dataset.key;
    if (!key) return nodes[lib] ?? null; // the library's root row
    if (row.dataset.isdir === "1") {
      const n = nodeAt(lib, key);
      return n && !n.lazy ? n.children : null;
    }
    const pk = parentKey(key);
    return pk ? (nodeAt(lib, pk)?.children ?? null) : (nodes[lib] ?? null);
  }

  /** What the dragged node (or the picked ones) really moves: not what is inside another moved folder,
   * not what is there already, not a folder into itself. */
  function moveList(l, node, key, target) {
    const group = multi.lib === l.id && multi.keys.size > 1 && multi.keys.has(key) ? pickedNodes(l).map((x) => x.node) : [node];
    const rels = group.filter((n) => n.rel != null && (n.path == null || isEditableName(n.rel))).map((n) => n.rel);
    return rels.filter((r) => !rels.some((o) => o !== r && r.startsWith(o + "/")) && parentOf(r) !== target && target !== r && !target.startsWith(r + "/"));
  }

  /** While dragging: can the nodes be dropped on this row? "ok", "no" (cannot: shown with the not-allowed pointer), or null (not a place for them). */
  function moveVerdict(l, node, key, row) {
    const rowLib = row.closest("[data-lib]")?.dataset.lib;
    if (!rowLib) return null;
    if (rowLib !== l.id) return "no"; // another library: moving between libraries is not possible (copy or link is still open)
    const target = dropTarget(row);
    const todo = moveList(l, node, key, target);
    if (!todo.length) return "no"; // nothing would change: it is there already, or inside itself
    const siblings = loadedChildren(l.id, row);
    if (siblings) {
      const taken = new Set(siblings.map((n) => lastOf(n.rel).toLowerCase()));
      if (todo.some((r) => taken.has(lastOf(r).toLowerCase()))) return "no"; // the name is used there
    }
    return "ok";
  }

  /** The dragged node (or the picked ones) was dropped on a row: move it into that folder (a file's row: its folder). */
  async function dropMove(l, node, key, row) {
    const target = dropTarget(row);
    const todo = moveList(l, node, key, target);
    if (!todo.length) return;
    const fails = [];
    for (const r of todo) {
      try {
        await relocate(l.id, { rel: r }, () => api.moveFolderNode(l.id, r, target));
      } catch (e) {
        fails.push(`${lastOf(r)}：${e}`);
      }
    }
    clearPicked();
    await reconcile(l.id, null);
    // show where they went
    const trail = target ? await findFolderNode(l.id, target) : [];
    if (trail) {
      let k = "";
      for (const i of trail) {
        k += `/${i}`;
        expansionOf(l.id)[k] = true;
      }
      await reconcile(l.id, null);
    }
    if (fails.length) onerror?.(fails.join("\n"));
  }

  // ---- selected text dropped on a node of the tree: it becomes a new note there

  /** A name for a note made of `text`: its first line, without what a file name cannot have. */
  function nameFrom(text) {
    const line = (text.split(/\r?\n/).find((s) => s.trim()) ?? "").replace(/[\\/:*?"<>|\u0000-\u001f]/g, " ").replace(/\s+/g, " ").trim();
    return Array.from(line).slice(0, 30).join("").replace(/[. ]+$/, "").replace(/^[. ]+/, "") || "笔记";
  }

  /** Can text be dropped on this row: only on a library that can be edited. */
  function textVerdict(row) {
    const lib = row.closest("[data-lib]")?.dataset.lib;
    if (!lib) return null;
    return isEditable(lib) ? "ok" : "no";
  }

  /** The text was dropped on a row: ask for the name of the note, then make it (inside the folder or note of the row; for a file, next to it). */
  function textDrop(row, text) {
    const lib = row.closest("[data-lib]")?.dataset.lib;
    if (!lib || !isEditable(lib)) return;
    const folderLib = isRwFolder(lib);
    const md = text.replace(/\r?\n+/g, "\n\n") + "\n";
    nameDlg = {
      title: "用选中的文字新建笔记",
      label: folderLib ? "文件名（不含后缀）" : "笔记标题",
      value: nameFrom(text),
      okLabel: "新建",
      hint: folderLib ? nameRules : "",
      validate: folderLib ? checkName : null,
      onok: guard(lib, async (title) => {
        let r;
        if (folderLib) {
          r = await api.createFolderFile(lib, dropTarget(row), "md", title);
        } else {
          const key = row.dataset.key;
          r = await api.createNoteAt(lib, key ? pathOf(lib, key) : "", title, "md");
        }
        await api.saveNote(lib, r.id, md);
        onlibs?.();
        await tick();
        await showNew(r);
        await workspace.openVolume({ lib: r.lib, path: r.id });
      }),
    };
  }

  $effect(() => installTextDrag({ check: textVerdict, onDrop: textDrop }));

  // The highlight of the node that was opened or revealed last gives way when another reading tab
  // becomes the active one (the active tab's own node is shown by `activeKey`): a node whose tab was
  // closed must not keep its background.
  let lastTab = "";
  $effect(() => {
    const c = workspace.ws.current;
    const id = c ? `${c.lib}|${c.bookId ?? ""}|${c.path ?? ""}` : "";
    untrack(() => {
      if (id !== lastTab) {
        lastTab = id;
        current = null;
      }
    });
  });

  // ---- keeping the tree of a local folder in step with the disk (like an IDE): when the window gets the
  // focus again, and whenever a folder is opened, what is shown is compared with the folder; nodes that are
  // gone leave, new ones join, and nothing that is open is closed.
  const reconciling = {};

  /** Compare one loaded level of a folder library (`key` null: the top) with the disk, then the open levels below it. */
  async function reconcile(lib, key) {
    const parent = key ? nodeAt(lib, key) : null;
    if (key && (!parent || parent.lazy)) return; // not loaded: it is read when it is opened
    const old = key ? parent.children : (nodes[lib] ?? []);
    let fresh;
    try {
      fresh = await api.folderChildren(lib, parent ? parent.rel : "");
    } catch {
      return; // the folder is gone or not readable: leave what is shown
    }
    const oldRels = old.map((n) => n.rel);
    const newRels = fresh.map((n) => n.rel);
    const same = oldRels.length === newRels.length && oldRels.every((r, i) => r === newRels[i]);
    if (!same) {
      remapKeys(lib, key ?? "", oldRels, newRels);
      const byRel = new Map(old.map((n) => [n.rel, n]));
      const merged = fresh.map((n) => {
        const o = byRel.get(n.rel);
        if (!o) return n;
        o.title = n.title; // a loaded folder keeps what was read below it
        return o;
      });
      if (key) parent.children = merged;
      else nodes[lib] = merged;
    }
    const level = key ?? "";
    const list = key ? nodeAt(lib, key).children : nodes[lib];
    const isNotesLib = isNotes(lib);
    for (let i = 0; i < list.length; i++) {
      const k = `${level}/${i}`;
      if ((isNotesLib || list[i].path == null) && !list[i].lazy && expanded[lib]?.[k]) await reconcile(lib, k);
    }
  }

  /** The nodes of a level changed places: what is kept by index path (open folders, the marked node) follows. */
  function remapKeys(lib, level, oldRels, newRels) {
    const prefix = `${level}/`;
    const map = (k) => {
      if (!k.startsWith(prefix)) return k;
      const rest = k.slice(prefix.length);
      const slash = rest.indexOf("/");
      const j = newRels.indexOf(oldRels[Number(slash < 0 ? rest : rest.slice(0, slash))]);
      return j < 0 ? null : prefix + j + (slash < 0 ? "" : rest.slice(slash));
    };
    const next = {};
    for (const k of Object.keys(expanded[lib] ?? {})) {
      const m = map(k);
      if (m) next[m] = true;
    }
    expanded[lib] = next;
    for (const ref of [current, selected]) {
      if (ref?.lib === lib) ref.key = map(ref.key) ?? "";
    }
    if (current?.lib === lib && !current.key) current = null;
    if (selected?.lib === lib && !selected.key) selected = null;
    if (multi.lib === lib) clearPicked();
  }

  async function syncFolders() {
    for (const l of libraries) {
      if (l.kind !== "folder" || !open[l.id] || !nodes[l.id] || reconciling[l.id]) continue;
      reconciling[l.id] = true;
      try {
        await reconcile(l.id, null);
      } finally {
        reconciling[l.id] = false;
      }
    }
    api.sidecarRefresh().catch(() => {}); // the file that travels with a folder may have been changed by git
  }

  $effect(() => {
    let t = null;
    const onFocus = () => {
      clearTimeout(t);
      t = setTimeout(syncFolders, 300);
    };
    window.addEventListener("focus", onFocus);
    return () => {
      window.removeEventListener("focus", onFocus);
      clearTimeout(t);
    };
  });

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

  /** The original file: a file path for folders, `<package>!/<path>` for data packages.
   *  `href` is not used: it may point at a file the package does not have, and an
   *  unresolved leaf is better served by the package path itself. */
  function filePath(l, node) {
    const p = node.path ?? node.rel ?? node.start ?? node.toc ?? node.nav;
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
    // read-write folders: new files go into a folder (or next to a file); only the files the program edits can be renamed or deleted
    const rw = l.kind === "folder" && l.writable;
    const rwFile = rw && node.path != null && isEditableName(node.rel);
    const label = `${l.title} › ${node.title}`;
    return sections(
      notes || rw
        ? [
            { label: "新建笔记", action: () => createNote(l.id, key, "md") },
            { label: "新建脑图", action: () => createNote(l.id, key, "mindmap") },
            { label: "新建白板", action: () => createNote(l.id, key, "whiteboard") },
            ...(rw ? [{ label: "新建文件夹…", action: () => newFolder(l.id, key) }] : []),
          ]
        : [],
      notes
        ? [
            { label: "重命名…（F2）", action: () => renameNote(l.id, node, key) },
            { label: "移动到…", action: () => moveNoteNode(l.id, node, key) },
            { label: "上移", action: () => shiftNote(l.id, node, key, -1), disabled: !canShift(l.id, node, key, -1) },
            { label: "下移", action: () => shiftNote(l.id, node, key, 1), disabled: !canShift(l.id, node, key, 1) },
            { label: "删除…", action: () => deleteNote(l.id, node, key) },
          ]
        : rwFile
          ? [
              { label: "重命名…（F2）", action: () => renameNote(l.id, node, key) },
              { label: "移动到…", action: () => moveNode(l.id, node, key) },
              // files are ordered by name, there is no moving up or down
              { label: "上移", action: () => {}, disabled: true },
              { label: "下移", action: () => {}, disabled: true },
              { label: "删除…", action: () => deleteNote(l.id, node, key) },
            ]
          : rw && node.path == null
            ? [
                { label: "重命名…（F2）", action: () => renameNote(l.id, node, key) },
                { label: "移动到…", action: () => moveNode(l.id, node, key) },
                { label: "删除文件夹…", action: () => deleteNote(l.id, node, key) },
              ]
            : [],
      [
        ...(notes ? [] : [{ label: "复制原文件路径", action: () => oncopy(filePath(l, node)) }]),
        { label: "复制目录树路径", action: () => oncopy(titleChain(l, key).join(" › ")) },
      ],
      [
        { label: "从这里搜索标题…", action: () => onsearchfrom("title", { lib: l.id, label, within: withinOf(l, node) }) },
        { label: "从这里搜索全文…", action: () => onsearchfrom("full", { lib: l.id, label, within: withinOf(l, node) }) },
        { label: "加入搜索范围", action: () => onsearchfrom("scope-add", { lib: l.id, label: titleChain(l, key).join(" › "), within: withinOf(l, node) }) },
        { label: "加入每日一句来源", action: () => onsearchfrom("daily-add", { lib: l.id, label: titleChain(l, key).join(" › "), within: withinOf(l, node) }) },
      ],
      node.book_id || node.path || node.rel || l.kind === "cbeta" || l.kind === "html"
        ? [
            { label: "属性…", action: () => onprops(l, node, key, pathOf(l.id, key)) },
            ...(node.book_id || node.path || node.rel ? [{ label: "标签…", action: () => ontag(l, node, pathOf(l.id, key)) }] : []),
          ]
        : [],
      [
        ...(notes ? [{ label: "导入到这里（作为子节点）…", action: () => onimport({ lib: l.id, parent: node.rel, key }) }] : []),
        { label: "导出此节点…", action: () => onexport(l, { whole: false, keys: [key], rels: node.rel ? [node.rel] : [] }, node.title, node.title, pathOf(l.id, key)) },
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
      { label: "新建笔记（Ctrl+Shift+1）", action: () => addNoteHere("md") },
      { label: "新建脑图（Ctrl+Shift+2）", action: () => addNoteHere("mindmap") },
      { label: "新建白板（Ctrl+Shift+3）", action: () => addNoteHere("whiteboard") },
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
  function expansionOf(lib) {
    // Read back the object after assignment to get Svelte's reactive proxy.
    if (!expanded[lib]) expanded[lib] = {};
    return expanded[lib];
  }
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
      onerror?.(String(e));
    }
  }

  async function toggleNode(lib, key) {
    const ex = expansionOf(lib);
    if (ex[key]) delete ex[key];
    else {
      // opening the current month of the diary makes sure that today's entry is there
      const n = nodeAt(lib, key);
      const libObj = libraries.find((l) => l.id === lib);
      const isCurDiaryMonth =
        (lib === DIARY_LIBRARY_ID && n?.rel === monthNodeId()) ||
        (libObj?.title === monthPath().split("/")[0] && pathOf(lib, key) === monthPath());
      if (isCurDiaryMonth) {
        try {
          const r = await api.diaryToday(localDate());
          await refresh(lib, key);
        } catch {}
      }
      await loadChildren(lib, n);
      ex[key] = true;
      // a folder that was read before is compared with the disk again
      if (libraries.find((l) => l.id === lib)?.kind === "folder" && n && n.path == null) reconcile(lib, key);
    }
  }

  /** Full tree path (`lib/a/b/node`) for a document when its branch is loaded. */
  export function nodePath(lib, { bookId = null, path = "" } = {}) {
    if (nodes[lib]) {
      const trail = findNode(nodes[lib], { bookId, path });
      if (trail) return pathOf(lib, "/" + trail.join("/"));
    }
    return "";
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
    const ex = expansionOf(lib);
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
    if (!l.index_ready) return { text: "未建索引", warn: true, index: true };
    if (l.index_data_version && l.data_version && l.index_data_version !== l.data_version)
      return { text: "索引过期", warn: true, index: true };
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
        notes || (l.kind === "folder" && l.writable)
          ? [
              { label: "新建笔记", action: () => createNote(l.id, null, "md") },
              { label: "新建脑图", action: () => createNote(l.id, null, "mindmap") },
              { label: "新建白板", action: () => createNote(l.id, null, "whiteboard") },
              ...(l.kind === "folder" ? [{ label: "新建文件夹…", action: () => newFolder(l.id, null) }] : []),
            ]
          : [],
        [{ label: "重命名…（F2）", action: () => renameLibrary(l) }, { label: "属性…", action: () => onprops(l, null, "", l.title) }],
        [
          ...(notes ? [] : [{ label: "复制原文件路径", action: () => oncopy(l.source) }]),
          { label: "复制目录树路径", action: () => oncopy(l.title) },
        ],
        [
          { label: "从这里搜索标题…", action: () => onsearchfrom("title", { lib: l.id, label: l.title, within: null }) },
          { label: "从这里搜索全文…", action: () => onsearchfrom("full", { lib: l.id, label: l.title, within: null }) },
          { label: "加入搜索范围", action: () => onsearchfrom("scope-add", { lib: l.id, label: l.title, within: null }), disabled: !l.index_ready },
          { label: "加入每日一句来源", action: () => onsearchfrom("daily-add", { lib: l.id, label: l.title, within: null }), disabled: !l.index_ready },
          { label: l.index_ready ? "重建索引" : "建立索引", action: () => onindex(l.id), disabled: building || !!l.error },
        ],
        [
          ...(notes ? [{ label: "导入到此知识库…", action: () => onimport({ lib: l.id, parent: null, key: null }) }] : []),
          { label: "导出知识库…", action: () => onexport(l, { whole: true, keys: [], rels: [] }, l.title, l.title, l.title) },
          ...(l.kind === "folder" && l.writable ? [{ label: "回收站…", action: () => (trashDlg = { lib: l.id, title: l.title }) }] : []),
          ...(notes ? [{ label: "回收站…", action: () => (notesTrashDlg = { lib: l.id, title: l.title }) }] : []),
          ...(l.kind === "folder"
            ? [l.writable ? { label: "锁定知识库（改为只读）", action: () => setWritable(l, false), disabled: building } : { label: "解锁知识库（可读写）…", action: () => setWritable(l, true), disabled: building }]
            : []),
          ...(l.kind === "folder" || l.kind === "zim" ? [] : [{ label: "绑定知识库…", action: () => onrebind(l) }]),
          { label: notes ? "删除知识库…" : "解除绑定", action: () => onunbind(l), disabled: building },
        ],
      ),
    };
  }

  /** Another name for a library; a bound one is only shown under it, its data stays as it is. */
  function renameLibrary(l) {
    nameDlg = {
      title: `重命名知识库：${l.title}`,
      nodePath: l.title,
      label: "名称",
      value: l.title,
      okLabel: "确定",
      onok: guard(l.id, async (title) => {
        await api.renameLibrary(l.id, title);
        await onlibs?.();
      }),
    };
  }

  /** Rename shortcut renames what has the focus in the tree (a library, or a note), else the last note opened from the tree. */
  function f2(e) {
    if (!matchesShortcut(e, "rename") || nameDlg) return;
    const t = e.target;
    if (t?.closest?.("input, textarea, select, [contenteditable], .ProseMirror, .excalidraw")) return;
    const lib = libraries.find((x) => x.id === t?.closest?.("[data-lib]")?.dataset.lib);
    const key = t?.closest?.("[data-key]")?.dataset.key;
    if (lib && !key) {
      e.preventDefault();
      return renameLibrary(lib);
    }
    const at = lib && key ? { lib: lib.id, key } : selected && isEditable(selected.lib) ? selected : null;
    const node = at ? nodeAt(at.lib, at.key) : null;
    // in a read-write folder only the files the program edits can be renamed
    if (node && (isNotes(at.lib) || (isRwFolder(at.lib) && (node.path == null || isEditableName(node.rel))))) {
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

<div class="panel" bind:this={panel} role="tree" aria-label="知识库" tabindex="-1" oncontextmenu={panelMenu}>
  {#each libraries as l (l.id)}
    {@const b = badge(l)}
    <div class="lib" data-lib={l.id}>
      <div class="root" class:marked={menuMark?.lib === l.id && menuMark.key === null} data-rel="" data-isdir="1" role="treeitem" aria-expanded={!!open[l.id]} aria-selected="false" tabindex="0"
        oncontextmenu={(e) => libMenu(e, l)}
        onpointerdown={(e) => dragNode(l, null, null, e)}
        onkeydown={(e) => (e.key === "Enter" || e.key === " ") && (e.preventDefault(), toggle(l))}>
        <button class="twisty" onclick={() => toggle(l)} tabindex="-1" aria-hidden="true">
          <Icon name={open[l.id] ? "chevron-down" : "chevron-right"} size={14} />
        </button>
        <span class="libicon" title={kindLabel(l)}><Icon name={kindIcon(l)} size={18} /></span>
        <button class="title" onclick={() => toggle(l)} tabindex="-1" title={`${l.title}\n${l.source}${l.kind === "folder" && l.writable ? "\n读写模式" : ""}`}>{l.title}</button>
        {#if b?.index}
          <button class="badge warn link" tabindex="-1" title="点击建立索引" onclick={(e) => (e.stopPropagation(), onindex(l.id))}>{b.text}</button>
        {:else if b}<span class="badge" class:warn={b.warn}>{b.text}</span>{/if}
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
            ondrag={(node, key, e) => dragNode(l, node, key, e)}
            onopen={(node, key) => {
              selected = { lib: l.id, key };
              current = { lib: l.id, key }; // the highlight follows what was opened last
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

{#if moveDlg}
  <MoveDialog {...moveDlg} onclose={() => (moveDlg = null)} />
{/if}

{#if trashDlg}
  <TrashDialog lib={trashDlg.lib} libTitle={trashDlg.title} onclose={() => (trashDlg = null)}
    onrestored={() => trashDlg && reconcile(trashDlg.lib, null)} />
{/if}

{#if notesTrashDlg}
  <NotesTrashDialog lib={notesTrashDlg.lib} libTitle={notesTrashDlg.title} onclose={() => (notesTrashDlg = null)}
    onrestored={(r, quiet = false) => {
      const lib = notesTrashDlg?.lib;
      if (r) return showNew(r, !quiet).catch((e) => onerror?.(String(e)));
      if (lib) return reconcile(lib, null);
    }} />
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
  .badge.link { border: none; background: none; padding: 1px 4px; border-radius: 4px; cursor: pointer; }
  .badge.link:hover { background: var(--accent-soft); text-decoration: underline; }
  .note { font-size: 13px; padding-left: 24px; }
  .error { color: var(--hl); }
  .empty { padding: 24px 12px; text-align: center; }
  .small { font-size: 13px; }
</style>
