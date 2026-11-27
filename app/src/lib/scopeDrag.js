// Dragging tree nodes into the search range list of a search page. Done with
// pointer events: HTML5 drag and drop is unreliable in the webview (the
// window's own file-drop handling can swallow it).
//
// A drop target is any element with `data-scope-drop`; it gets a `scopedrop`
// event whose `detail` is the list of search range items.

let ghost = null;

/**
 * Call on `pointerdown` of a tree row. Nothing happens until the pointer has
 * moved a few pixels (a plain click stays a click); `getItems()` then gives
 * the items to carry: [{ lib, label, books, paths, rels, whole }].
 */
export function startScopeDrag(e, getItems, moveTo = null) {
  if (e.button !== 0 || e.ctrlKey || e.metaKey || e.shiftKey || e.altKey) return;
  const x0 = e.clientX;
  const y0 = e.clientY;
  let items = null;
  let dragging = false;
  let over = null;
  let moveOver = null;

  let moveNo = false;
  let moveZone = "in";
  const setMove = (el, no = false, zone = "in") => {
    if (el === moveOver && no === moveNo && zone === moveZone) return;
    moveOver?.classList.remove("move-over", "move-no", "move-before", "move-after");
    if (el) el.classList.add(no ? "move-no" : zone === "before" ? "move-before" : zone === "after" ? "move-after" : "move-over");
    moveZone = zone;
    document.body.classList.toggle("move-blocked", !!el && no); // the pointer says it cannot be dropped here
    moveOver = el;
    moveNo = no;
  };
  const setOver = (el) => {
    if (el === over) return;
    over?.classList.remove("scope-over");
    if (el && (!el.dataset.scopeLib || items.every((item) => item.lib === el.dataset.scopeLib))) {
      el.classList.add("scope-over");
    }
    over = el;
  };
  const move = (ev) => {
    if (!dragging) {
      if (Math.hypot(ev.clientX - x0, ev.clientY - y0) < 6) return;
      items = getItems();
      if (!items?.length) return stop();
      dragging = true;
      ghost = document.createElement("div");
      ghost.className = "scope-ghost";
      ghost.textContent = items.length > 1 ? `${items.length} 项：${items[0].label}…` : items[0].label;
      document.body.appendChild(ghost);
      document.body.classList.add("scope-dragging");
    }
    ghost.style.left = `${ev.clientX + 12}px`;
    ghost.style.top = `${ev.clientY + 12}px`;
    const under = document.elementFromPoint(ev.clientX, ev.clientY);
    setOver(under?.closest("[data-scope-drop]") ?? null);
    // a row of the tree to drop on (moving the dragged nodes into that folder, or reordering)
    const row = moveTo ? under?.closest(moveTo.selector ?? "[data-rel]") : null;
    // `reorder`: the top / bottom quarter of a row means before / after it, the middle means into it;
    // `"only"`: top / bottom half means before / after (no "into" zone)
    let zone = "in";
    if (row && moveTo.reorder === "only") {
      const r = row.getBoundingClientRect();
      const f = (ev.clientY - r.top) / Math.max(1, r.height);
      zone = f < 0.5 ? "before" : "after";
    } else if (row && moveTo.reorder && row.dataset.key) {
      const r = row.getBoundingClientRect();
      const f = (ev.clientY - r.top) / Math.max(1, r.height);
      zone = f < 0.25 ? "before" : f > 0.75 ? "after" : "in";
    }
    const verdict = row ? moveTo.check(row, zone) : null; // "ok", "no" or null (not a place to drop on)
    setMove(verdict ? row : null, verdict === "no", zone);
  };
  const up = () => {
    const target = over;
    const into = moveNo ? null : moveOver;
    const zone = moveZone;
    const carried = items;
    const was = dragging;
    stop();
    if (was) {
      // the click that ends the drag must not open the node
      const swallow = (c) => {
        c.stopPropagation();
        c.preventDefault();
      };
      window.addEventListener("click", swallow, { capture: true, once: true });
      setTimeout(() => window.removeEventListener("click", swallow, true), 0);
      if (target) target.dispatchEvent(new CustomEvent("scopedrop", { detail: carried }));
      else if (into) moveTo.onDrop(into, zone);
    }
  };
  const key = (ev) => ev.key === "Escape" && stop();
  function stop() {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    window.removeEventListener("pointercancel", stop);
    window.removeEventListener("keydown", key);
    setOver(null);
    setMove(null);
    ghost?.remove();
    ghost = null;
    document.body.classList.remove("scope-dragging");
  }
  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up);
  window.addEventListener("pointercancel", stop);
  window.addEventListener("keydown", key);
}
