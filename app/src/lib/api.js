// Thin wrappers around the Tauri commands (app/src-tauri/src/commands.rs).
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export const status = () => invoke("status");
export const inspectSource = (path) => invoke("inspect_source", { path });
export const bindLibraries = (path, cbetaTree, htmlTrees, libraryIds = {}) =>
  invoke("bind_libraries", { path, cbetaTree, htmlTrees, libraryIds });
export const inspectFolder = (path) => invoke("inspect_folder", { path });
export const addFolder = (path, title, exts, maxMb, writable = false, libraryId = "") => invoke("add_folder", { path, title, exts, maxMb, writable, libraryId });
/** Read-write mode of a folder library on or off (ask the user to accept the rules first: writable.js). */
export const setFolderWritable = (id, writable) => invoke("set_folder_writable", { id, writable });
export const addZim = (path, libraryId = "") => invoke("add_zim", { path, libraryId });
/** Header-only check of a ZIM file: { major, minor, entries, clusters, bytes }. */
export const inspectZim = (path) => invoke("inspect_zim", { path });
export const resolveZimLink = (lib, path, href) => invoke("resolve_zim_link", { lib, path, href });
export const openExternalUrl = (url) => invoke("open_external_url", { url });
export const openExternalWindow = (url, aiOn = false) => invoke("open_external_window", { url, aiOn });
export const openInFileManager = (lib, rel = null) => invoke("open_in_file_manager", { lib, rel });
/** A new note / mind map / whiteboard file (`kind`: md, mindmap, whiteboard) in a folder (`parent`: path, "" for the top). */
export const createFolderFile = (lib, parent, kind = "md", title = null) => invoke("create_folder_file", { lib, parent, kind, title });
/** A new folder inside a folder (`parent`: path, "" for the top) of a read-write folder library. */
export const createFolderDir = (lib, parent, name) => invoke("create_folder_dir", { lib, parent, name });
/** Move a file or folder into another folder ("" for the top); resolves to its new path. */
export const moveFolderNode = (lib, rel, parent) => invoke("move_folder_node", { lib, rel, parent });
/** Every folder of a read-write folder library (paths). */
export const folderDirs = (lib) => invoke("folder_dirs", { lib });
/** Delete a folder with everything in it (to the trash inside .smart-library); resolves to the number of documents in it. */
export const deleteFolderDir = (lib, rel) => invoke("delete_folder_dir", { lib, rel });
/** Deleted files and folders of a read-write folder library: `{ entry, rel, kind, time }`, newest first. */
export const trashList = (lib) => invoke("trash_list", { lib });
/** Put a trashed file or folder back at its old place; resolves to `{ rel, pictures }`. */
export const trashRestore = (lib, entry) => invoke("trash_restore", { lib, entry });
/** Rejects with the reason when a path of folders (`a/b`) has a bad name in it. */
export const checkFilePath = (path) => invoke("check_file_path", { path });
/** Read again the file that travels with a read-write folder when it was changed outside; resolves to true when it was. */
export const sidecarRefresh = () => invoke("sidecar_refresh");
/** Empty the trash of read-write local folders and delete pictures no note refers to; resolves to a sentence. */
export const clearCache = () => invoke("clear_cache");
export const t2sPairs = () => invoke("t2s_pairs");
export const readTextFile = (path) => invoke("read_text_file", { path });
export const dynastyTable = () => invoke("dynasty_table");
export const dynastyText = () => invoke("dynasty_text");
export const setDynastyText = (text) => invoke("set_dynasty_text", { text });
/** The rules for local file names, as text. */
export const fileNameRules = () => invoke("file_name_rules");
/** Rejects with the reason when `name` cannot be a local file name. */
export const checkFileName = (name) => invoke("check_file_name", { name });
export const folderChildren = (lib, rel) => invoke("folder_children", { lib, rel });
export const createNotesLibrary = (title, libraryId = "") => invoke("create_notes_library", { title, libraryId });
export const createNote = (lib, parent, title) => invoke("create_note", { lib, parent, title });
export const noteTargets = (query) => invoke("note_targets", { query });
export const createNoteAt = (defaultLib, position, title = null, kind = null) => invoke("create_note_at", { defaultLib, position, title, kind });
export const renameLibrary = (id, title) => invoke("rename_library", { id, title });
export const reorderLibraries = (ids) => invoke("reorder_libraries", { ids });
export const noteChain = (lib, id) => invoke("note_chain", { lib, id });
export const diaryToday = (date) => invoke("diary_today", { date });
export const mindmapAddClip = (lib, id, topic, link, tag) => invoke("mindmap_add_clip", { lib, id, topic, link, tag });
export const recentTouch = (lib, path, bookId, title) => invoke("recent_touch", { lib, path, bookId, title });
export const recentBump = (lib, path) => invoke("recent_bump", { lib, path });
export const recentProgress = (lib, path, lb, mode) => invoke("recent_progress", { lib, path, lb, mode });
export const recentPatchExtra = (lib, path, patch, title = null) =>
  invoke("recent_patch_extra", { lib, path, patch, title });
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
export const propsGet = (lib, bookId, path) => invoke("props_get", { lib, bookId, path });
export const propsSet = (lib, bookId, path, dims, aliases = null) => invoke("props_set", { lib, bookId, path, dims, aliases });
export const appInfo = () => invoke("app_info");
export const listFonts = () => invoke("list_fonts");
export const findInPage = () => invoke("find_in_page");
export const onExportProgress = (cb) => listen("export-progress", (e) => cb(e.payload));
export const exportLibrary = (req) => invoke("export_library", { req });
export const inspectPackage = (path) => invoke("inspect_package", { path });
export const importPackage = (path, lib = null, parent = null, title = null, libraryId = "") =>
  invoke("import_package", { path, lib, parent, title, libraryId });
export const importDirAsFolder = (path, dest, title = null, writable = false, libraryId = "") =>
  invoke("import_dir_as_folder", { path, dest, title, writable, libraryId });
export const gitCheck = (customPath = null) => invoke("git_check", { customPath });
export const gitDefaultDir = (url) => invoke("git_default_dir", { url });
export const addGitLibrary = (url, dest, title, branch, exts, maxMb, libraryId = "") =>
  invoke("add_git_library", { url, dest, title, branch, exts, maxMb, libraryId });
export const gitStatusAll = (fetch = false) => invoke("git_status_all", { fetch });
export const gitPullLibrary = (id) => invoke("git_pull_library", { id });
export const renameNote = (lib, id, title) => invoke("rename_note", { lib, id, title });
/** Keep a picture (an ArrayBuffer) for the notes of a library; resolves to its name. */
/** Keep a picture for a note; `path`: the note (a file of a local folder keeps its pictures beside it). */
export const noteAttach = (lib, bytes, mime, path = "") => invoke("note_attach", bytes, { headers: { lib, mime, path: encodeURIComponent(path) } });
/** Write bytes (a Uint8Array) to a file the user chose. */
export const saveBytes = (path, bytes) => invoke("save_bytes", bytes, { headers: { path: encodeURIComponent(path) } });
/** The bytes (an ArrayBuffer) of a picture of a library's notes. */
export const notePicture = (lib, name) => invoke("note_picture", { lib, name });
export const noteAppend = (lib, id, text) => invoke("note_append", { lib, id, text });
export const saveNote = (lib, id, content) => invoke("save_note", { lib, id, content });
export const deleteNote = (lib, id) => invoke("delete_note", { lib, id });
export const noteMoveTargets = (lib, id) => invoke("note_move_targets", { lib, id });
export const moveNote = (lib, id, parent, index) => invoke("move_note", { lib, id, parent, index });
export const unbindLibrary = (id, deleteLocal = false) => invoke("unbind_library", { id, deleteLocal });
export const tree = (lib) => invoke("tree", { lib });
export const openNode = (lib, bookId, path, query = null, phrase = false) =>
  invoke("open_node", { lib, bookId, path, query, phrase });
export const folderFileStamp = (lib, path) => invoke("folder_file_stamp", { lib, path });
export const resolveNode = (lib, bookId = null, path = null) => invoke("resolve_node", { lib, bookId, path });
/** The original text of a document, for the raw data window. */
export const rawSource = (lib, path = null, bookId = null) => invoke("raw_source", { lib, path, bookId });
export const search = (args) => invoke("search", args);
export const randomSentence = (args) => invoke("random_sentence", args);
export const quickSearch = (query, lib = null, limit = 200, quote = null, within = null, pinyin = false) =>
  invoke("quick_search", { query, lib, limit, quote, within, pinyin });
export const citationCommand = (text) => invoke("citation_command", { text });
export const prepareTitles = () => invoke("prepare_titles");
export const resolveBookRefs = (lib, path = null, bookId = null, titles = []) =>
  invoke("resolve_book_refs", { lib, path, bookId, titles });
export const toPinyin = (text) => invoke("to_pinyin", { text });
export const pinyinConvert = (text) => invoke("pinyin_convert", { text });
export const pinyinText = () => invoke("pinyin_text");
export const setPinyinText = (text) => invoke("set_pinyin_text", { text });
export const buildIndex = (lib) => invoke("build_index", { lib });
export const checkFolderChanges = () => invoke("check_folder_changes");
export const onIndexProgress = (fn) => listen("index-progress", (e) => fn(e.payload));
export const onIndexDone = (fn) => listen("index-done", (e) => fn(e.payload));
/** Reading and binding a library: { label, done, total } (total 0 = unknown). */
export const onBindProgress = (fn) => listen("bind-progress", (e) => fn(e.payload));
export const cancelBind = () => invoke("cancel_bind");
/** A library's title list being built: { lib, done, total } (total 0 = starting). */
export const onTitlesProgress = (fn) => listen("titles-progress", (e) => fn(e.payload));
export const onTitlesDone = (fn) => listen("titles-done", (e) => fn(e.payload));
export const loadWorkspace = () => invoke("load_workspace");
export const saveWorkspace = (workspace) => invoke("save_workspace", { workspace });
export const aiStream = (reqId, config, system, messages, reasoningEffort = null, tools = null) =>
  invoke("ai_stream", { reqId, config, system, messages, reasoningEffort, tools });
export const aiAbort = (reqId) => invoke("ai_abort", { reqId });
export const aiToolDefinitions = () => invoke("ai_tool_definitions");
export const aiExecTool = (name, argumentsJson) =>
  invoke("ai_exec_tool", { name, arguments: argumentsJson });
export const aiTest = (config) => invoke("ai_test", { config });
export const aiFetchModels = (format, baseUrl, apiKey) =>
  invoke("ai_fetch_models", { format, baseUrl, apiKey });
export const aiSummarizeTitle = (config, userText, assistantText) =>
  invoke("ai_summarize_title", { config, userText, assistantText });
export const aiSummarizeNoteTitle = (config, content) =>
  invoke("ai_summarize_note_title", { config, content });
export const onAiStream = (fn) => listen("ai-stream", (e) => fn(e.payload));
export const aiMindmapParse = (input, fallbackTitle = "脑图") =>
  invoke("ai_mindmap_parse", { input, fallbackTitle });
export const aiDocText = (lib, bookId = null, path = null) =>
  invoke("ai_doc_text", { lib, bookId, path });
export const aiSessionSave = (args) => invoke("ai_session_save", args);
export const aiSessionGet = (id) => invoke("ai_session_get", { id });
export const aiSessionLatest = (lib, path) => invoke("ai_session_latest", { lib, path });
export const aiSessionList = (lib = null, path = null, limit = 100) =>
  invoke("ai_session_list", { lib, path, limit });
export const aiSessionDelete = (id) => invoke("ai_session_delete", { id });
export const aiSessionRename = (id, title) => invoke("ai_session_rename", { id, title });
export const aiSessionClear = () => invoke("ai_session_clear");

let aiSeq = 0;
/**
 * Stream an LLM response as an AsyncGenerator<string> (compatible with
 * `@milkdown/crepe`'s `AIProvider` and general streaming helpers).
 */
export async function* aiStreamGenerator(config, system, messages, signal = null) {
  const reqId = `ai-${Date.now()}-${++aiSeq}`;
  const queue = [];
  let done = false;
  let err = null;
  let wake = null;
  const notify = () => {
    if (wake) {
      const r = wake;
      wake = null;
      r();
    }
  };
  const unlisten = await onAiStream((ev) => {
    if (ev.req_id !== reqId) return;
    if (ev.delta) queue.push(ev.delta);
    if (ev.error) err = new Error(ev.error);
    if (ev.done) done = true;
    notify();
  });
  const onAbort = () => {
    done = true;
    aiAbort(reqId).catch(() => {});
    notify();
  };
  signal?.addEventListener("abort", onAbort, { once: true });
  aiStream(reqId, config, system, messages).catch((e) => {
    err = e instanceof Error ? e : new Error(String(e));
    done = true;
    notify();
  });
  try {
    while (true) {
      while (queue.length) yield queue.shift();
      if (err) throw err;
      if (done || signal?.aborted) break;
      await new Promise((r) => (wake = r));
    }
  } finally {
    signal?.removeEventListener("abort", onAbort);
    unlisten();
  }
}

// ------------------------------------------------------------ dictionaries
export const dictConfigGet = () => invoke("dict_config_get");
export const dictConfigSet = (paths, disabled) => invoke("dict_config_set", { paths, disabled });
export const dictReload = () => invoke("dict_reload");
export const dictSearch = (query, scopeIds = null, offset = 0, limit = 50) =>
  invoke("dict_search", { query, scopeIds, offset, limit });
export const dictEntry = (dictId, entryId) => invoke("dict_entry", { dictId, entryId });
export const dictLookup = (word, scopeIds = null, limit = 50) =>
  invoke("dict_lookup", { word, scopeIds, limit });
export const dictImport = (paths) => invoke("dict_import", { paths });

// ------------------------------------------------------------ snapshots
export const listSnapshots = (lib, docId) => invoke("list_snapshots", { lib, docId });
export const createSnapshot = (lib, docId, kind = "manual", label = null, content = null) =>
  invoke("create_snapshot", { lib, docId, kind, label, content });
export const getSnapshot = (id) => invoke("get_snapshot", { id });
export const restoreSnapshot = (id) => invoke("restore_snapshot", { id });
export const deleteSnapshot = (id) => invoke("delete_snapshot", { id });
/** Deleted notes of an internal library (its trash): `{ id, title, count, created_at, size_bytes }`. */
export const listDeletedNotes = (lib) => invoke("list_deleted_notes", { lib });
/** Put a deleted note and its subtree back; resolves to `{ lib, id, chain, title, count }`. */
export const restoreDeletedNote = (lib, id) => invoke("restore_deleted_note", { lib, id });
/** Throw away every deleted note of a library for good. */
export const purgeDeletedNotes = (lib) => invoke("purge_deleted_notes", { lib });
export const getSnapshotPolicy = () => invoke("get_snapshot_policy");
export const setSnapshotPolicy = (aiMaxPerDoc, aiTtlDays) =>
  invoke("set_snapshot_policy", { aiMaxPerDoc, aiTtlDays });

// ------------------------------------------------------------ local Agent (MCP)
export const mcpStatusGet = () => invoke("mcp_status_get");
export const mcpConfigSet = (config) => invoke("mcp_config_set", { config });
