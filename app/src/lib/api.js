// Thin wrappers around the Tauri commands (app/src-tauri/src/commands.rs).
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export const status = () => invoke("status");
export const inspectSource = (path) => invoke("inspect_source", { path });
export const bindLibraries = (path, cbetaTree, htmlTrees) =>
  invoke("bind_libraries", { path, cbetaTree, htmlTrees });
export const inspectFolder = (path) => invoke("inspect_folder", { path });
export const addFolder = (path, title, exts, maxMb) => invoke("add_folder", { path, title, exts, maxMb });
export const folderChildren = (lib, rel) => invoke("folder_children", { lib, rel });
export const createNotesLibrary = (title) => invoke("create_notes_library", { title });
export const createNote = (lib, parent, title) => invoke("create_note", { lib, parent, title });
export const noteTargets = (query) => invoke("note_targets", { query });
export const createNoteAt = (defaultLib, position, title, kind = null) => invoke("create_note_at", { defaultLib, position, title, kind });
export const mindmapAddClip = (lib, id, topic, link, tag) => invoke("mindmap_add_clip", { lib, id, topic, link, tag });
export const recentTouch = (lib, path, bookId, title) => invoke("recent_touch", { lib, path, bookId, title });
export const recentProgress = (lib, path, lb, mode) => invoke("recent_progress", { lib, path, lb, mode });
export const recentList = (filter = "", limit = 300) => invoke("recent_list", { filter, limit });
export const openedSet = (lib) => invoke("opened_set", { lib });
export const recentRemove = (lib, path) => invoke("recent_remove", { lib, path });
export const recentClear = () => invoke("recent_clear");
export const citeText = (lib, path, bookId, lb, title) => invoke("cite_text", { lib, path, bookId, lb, title });
export const markAdd = (m) => invoke("mark_add", m);
export const markUpdate = (id, note = null, tags = null) => invoke("mark_update", { id, note, tags });
export const markDelete = (id) => invoke("mark_delete", { id });
export const markList = (f = {}) => invoke("mark_list", f);
export const markTags = () => invoke("mark_tags");
export const nodeTagsGet = (lib, bookId, path) => invoke("node_tags_get", { lib, bookId, path });
export const nodeTagsSet = (lib, bookId, path, docTitle, tags) => invoke("node_tags_set", { lib, bookId, path, docTitle, tags });
export const appInfo = () => invoke("app_info");
export const listFonts = () => invoke("list_fonts");
export const findInPage = () => invoke("find_in_page");
export const onExportProgress = (cb) => listen("export-progress", (e) => cb(e.payload));
export const exportLibrary = (req) => invoke("export_library", { req });
export const importPackage = (path, lib = null, parent = null) => invoke("import_package", { path, lib, parent });
export const renameNote = (lib, id, title) => invoke("rename_note", { lib, id, title });
/** Keep a picture (an ArrayBuffer) for the notes of a library; resolves to its name. */
export const noteAttach = (lib, bytes, mime) => invoke("note_attach", bytes, { headers: { lib, mime } });
export const noteAppend = (lib, id, text) => invoke("note_append", { lib, id, text });
export const saveNote = (lib, id, content) => invoke("save_note", { lib, id, content });
export const deleteNote = (lib, id) => invoke("delete_note", { lib, id });
export const moveNote = (lib, id, parent, index) => invoke("move_note", { lib, id, parent, index });
export const unbindLibrary = (id) => invoke("unbind_library", { id });
export const tree = (lib) => invoke("tree", { lib });
export const openNode = (lib, bookId, path, query = null, phrase = false) =>
  invoke("open_node", { lib, bookId, path, query, phrase });
export const resolveNode = (lib, bookId = null, path = null) => invoke("resolve_node", { lib, bookId, path });
export const search = (args) => invoke("search", args);
export const quickSearch = (query, lib = null, limit = 200, quote = null, within = null) =>
  invoke("quick_search", { query, lib, limit, quote, within });
export const citationCommand = (text) => invoke("citation_command", { text });
export const prepareTitles = () => invoke("prepare_titles");
export const buildIndex = (lib) => invoke("build_index", { lib });
export const onIndexProgress = (fn) => listen("index-progress", (e) => fn(e.payload));
export const onIndexDone = (fn) => listen("index-done", (e) => fn(e.payload));
export const loadWorkspace = () => invoke("load_workspace");
export const saveWorkspace = (workspace) => invoke("save_workspace", { workspace });
