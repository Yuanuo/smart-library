// Thin wrappers around the Tauri commands (app/src-tauri/src/commands.rs).
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export const status = () => invoke("status");
export const inspectSource = (path) => invoke("inspect_source", { path });
export const bindLibraries = (path, cbetaTree, htmlTrees) =>
  invoke("bind_libraries", { path, cbetaTree, htmlTrees });
export const unbindLibrary = (id) => invoke("unbind_library", { id });
export const tree = (lib) => invoke("tree", { lib });
export const openNode = (lib, bookId, path, query = null, phrase = false) =>
  invoke("open_node", { lib, bookId, path, query, phrase });
export const resolveNode = (lib, bookId = null, path = null) => invoke("resolve_node", { lib, bookId, path });
export const search = (args) => invoke("search", args);
export const quickSearch = (query, lib = null, limit = 200, quote = null) =>
  invoke("quick_search", { query, lib, limit, quote });
export const citationCommand = (text) => invoke("citation_command", { text });
export const prepareTitles = () => invoke("prepare_titles");
export const buildIndex = (lib) => invoke("build_index", { lib });
export const onIndexProgress = (fn) => listen("index-progress", (e) => fn(e.payload));
export const onIndexDone = (fn) => listen("index-done", (e) => fn(e.payload));
export const loadWorkspace = () => invoke("load_workspace");
export const saveWorkspace = (workspace) => invoke("save_workspace", { workspace });
