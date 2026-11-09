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
export function startScopeDrag(e, getItems) {
  if (e.button !== 0 || e.ctrlKey || e.metaKey || e.shiftKey || e.altKey) return;
  const x0 = e.clientX;
  const y0 = e.clientY;
  let items = null;
  let dragging = false;
  let over = null;

  const setOver = (el) => {
    if (el === over) return;
    over?.classList.remove("scope-over");
    el?.classList.add("scope-over");
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
    setOver(document.elementFromPoint(ev.clientX, ev.clientY)?.closest("[data-scope-drop]") ?? null);
  };
  const up = () => {
    const target = over;
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
    }
  };
  const key = (ev) => ev.key === "Escape" && stop();
  function stop() {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    window.removeEventListener("pointercancel", stop);
    window.removeEventListener("keydown", key);
    setOver(null);
    ghost?.remove();
    ghost = null;
    document.body.classList.remove("scope-dragging");
  }
  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up);
  window.addEventListener("pointercancel", stop);
  window.addEventListener("keydown", key);
}
