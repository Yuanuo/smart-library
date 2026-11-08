// Keyboard shortcuts shared by the main window and the pop-out readers:
//   Ctrl/Cmd+G or Shift twice  title quick search
//   Ctrl/Cmd+H                 full-text search
//   Ctrl/Cmd+E / B / ,         recent / collections and tags / settings
//   Ctrl/Cmd+J                 open today's diary
// `fire(type)` is called with "quick", "search", "recent", "marks", "diary" or
// "settings". Returns a function that removes the listeners.

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
