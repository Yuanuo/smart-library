// Keyboard shortcuts shared by the main window and the pop-out readers:
//   Ctrl/Cmd+G or Shift twice  title quick search
//   Ctrl/Cmd+H                 full-text search
//   Ctrl/Cmd+E / B / ,         recent / collections and tags / settings
//   Ctrl/Cmd+J                 open today's diary
//   Ctrl/Cmd+T                 tags of the document being read or edited
//   Ctrl/Cmd+D                 bookmark here (the line being read)
//   Ctrl/Cmd+P                 properties (filters, other names, tags) of the document
//   Ctrl/Cmd+L                 show the document in the tree
//   Ctrl/Cmd+N                 new note
//   Ctrl/Cmd+W                 close the tab
//   Ctrl/Cmd+Tab (+Shift)      next / previous tab
//   Ctrl/Cmd+Shift+S           original / simplified characters
// `fire(type)` is called with "quick", "search", "recent", "marks", "diary",
// "settings", "tag", "bookmark", "props", "locate", "new", "close", "next-tab",
// "prev-tab" or "script". Returns a function that removes the listeners.

/** The text selected in a reader (not in an input), else "". */
export function readerSelection() {
  const s = window.getSelection();
  if (!s || s.isCollapsed || !s.rangeCount) return "";
  const n = s.anchorNode;
  const el = n?.nodeType === 1 ? n : n?.parentElement;
  if (!el?.closest?.(".reader")) return "";
  return s.toString().trim();
}

export function installShortcuts(fire) {
  let lastShift = 0;
  let shiftAlone = false;
  const keys = { g: "quick", h: "search", e: "recent", b: "marks", ",": "settings", j: "diary" };
  // about the document: not where a drawing board or a mind map has its own use of the keys
  const docKeys = { t: "tag", d: "bookmark", p: "props", l: "locate", n: "new", w: "close" };
  const ownKeys = ".excalidraw, .mind-elixir, .map-container";

  function down(e) {
    if ((e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey) {
      const type = keys[e.key.toLowerCase()];
      if (type) {
        e.preventDefault();
        shiftAlone = false;
        fire(type);
        return;
      }
    }
    if ((e.ctrlKey || e.metaKey) && !e.altKey) {
      let type = null;
      if (!e.shiftKey) type = docKeys[e.key.toLowerCase()];
      if (type && e.target?.closest?.(ownKeys)) type = null;
      if (e.key === "Tab") type = e.shiftKey ? "prev-tab" : "next-tab";
      if (e.shiftKey && e.key.toLowerCase() === "s") type = "script";
      if (type) {
        e.preventDefault();
        shiftAlone = false;
        fire(type);
        return;
      }
    }
    if (e.key === "Shift") {
      if (!e.repeat) shiftAlone = true;
    } else {
      shiftAlone = false;
      lastShift = 0;
    }
  }

  // Shift pressed twice (no other key in between, within 400 ms)
  function up(e) {
    if (e.key !== "Shift" || !shiftAlone) return;
    const now = performance.now();
    if (now - lastShift < 400) {
      lastShift = 0;
      fire("quick");
    } else {
      lastShift = now;
    }
  }

  window.addEventListener("keydown", down, true);
  window.addEventListener("keyup", up, true);
  return () => {
    window.removeEventListener("keydown", down, true);
    window.removeEventListener("keyup", up, true);
  };
}
