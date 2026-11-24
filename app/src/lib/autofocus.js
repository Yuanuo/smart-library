// Svelte action: focus the primary input when a dialog or wizard step mounts.
export function autofocus(node, opts = {}) {
  queueMicrotask(() => {
    if (!node?.isConnected || node.disabled) return;
    node.focus();
    if (opts?.select && typeof node.select === "function") node.select();
  });
}
