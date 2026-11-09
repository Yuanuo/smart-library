// Dragging selected text (from a reader or an editor) onto a node of the tree. The
// browser's own drag of selected text cannot reach the tree here (the window's
// file-drop handling), so it is done with pointer events, like `scopeDrag.js`.
//
// `install({ check, onDrop })`: `check(row)` says whether the row under the
// pointer can take the text: "ok", "no" (shown as not allowed), or null (not a
// place for it); `onDrop(row, text)` is called when it is dropped on an "ok" row.
// A row is any element with `data-rel` (the rows of the tree). Returns a function
// that removes it again.

export function installTextDrag({ check, onDrop }) {
  let ghost = null;
  let shown = null; // the row with a mark on it
  let blocked = false;

  const mark = (row, no) => {
    if (row === shown && no === blocked) return;
    shown?.classList.remove("move-over", "move-no");
    if (row) row.classList.add(no ? "move-no" : "move-over");
    document.body.classList.toggle("move-blocked", !!row && no);
    shown = row;
    blocked = no;
  };

  /** The pointer is on the selected text (not on a field, the tree or a menu). */
  const onSelection = (e) => {
    const sel = window.getSelection();
    if (!sel || sel.isCollapsed || sel.rangeCount === 0 || !sel.toString().trim()) return false;
    const host = sel.anchorNode?.nodeType === 1 ? sel.anchorNode : sel.anchorNode?.parentElement;
    if (!host || host.closest("input, textarea, [data-lib], .ctx-menu, dialog, [role='dialog']")) return false;
    for (const r of sel.getRangeAt(0).getClientRects()) {
      if (e.clientX >= r.left && e.clientX <= r.right && e.clientY >= r.top && e.clientY <= r.bottom) return true;
    }
    return false;
  };

  function down(e) {
    if (e.button !== 0 || e.ctrlKey || e.metaKey || e.shiftKey || e.altKey || !onSelection(e)) return;
    const text = window.getSelection().toString().trim();
    const x0 = e.clientX;
    const y0 = e.clientY;
    let dragging = false;
    let verdict = null;
    let row = null;
    e.preventDefault(); // the browser would start its own drag of the text

    const move = (ev) => {
      if (!dragging) {
        if (Math.hypot(ev.clientX - x0, ev.clientY - y0) < 6) return;
        dragging = true;
        ghost = document.createElement("div");
        ghost.className = "scope-ghost";
        ghost.textContent = text.length > 24 ? `${text.slice(0, 24)}…` : text;
        document.body.appendChild(ghost);
        document.body.classList.add("scope-dragging");
      }
      ghost.style.left = `${ev.clientX + 12}px`;
      ghost.style.top = `${ev.clientY + 12}px`;
      row = document.elementFromPoint(ev.clientX, ev.clientY)?.closest("[data-rel]") ?? null;
      verdict = row ? check(row) : null;
      mark(verdict ? row : null, verdict === "no");
    };
    const up = () => {
      const target = verdict === "ok" ? row : null;
      const was = dragging;
      stop();
      if (!was) {
        window.getSelection()?.removeAllRanges(); // a click in the selection puts the caret there
        return;
      }
      const swallow = (c) => {
        c.stopPropagation();
        c.preventDefault();
      };
      window.addEventListener("click", swallow, { capture: true, once: true });
      setTimeout(() => window.removeEventListener("click", swallow, true), 0);
      if (target) onDrop(target, text);
    };
    const key = (ev) => ev.key === "Escape" && stop();
    function stop() {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", stop);
      window.removeEventListener("keydown", key);
      mark(null, false);
      ghost?.remove();
      ghost = null;
      document.body.classList.remove("scope-dragging");
    }
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", stop);
    window.addEventListener("keydown", key);
  }

  document.addEventListener("mousedown", down, true);
  return () => document.removeEventListener("mousedown", down, true);
}
