// Thin wrappers around the Tauri commands (app/src-tauri/src/commands.rs).
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export const status = () => invoke("status");
export const openBookcase = (path) => invoke("open_bookcase", { path });
export const tree = (id) => invoke("tree", { id });
export const openVolume = (bookId, path = null, query = null, phrase = false) =>
  invoke("open_volume", { bookId, path, query, phrase });
export const search = (args) => invoke("search", args);
export const buildIndex = () => invoke("build_index");
export const onIndexProgress = (fn) => listen("index-progress", (e) => fn(e.payload));
export const onIndexDone = (fn) => listen("index-done", (e) => fn(e.payload));
