<script>
  // Pick the folder to move a file or folder into (a read-write local folder).
  import { onMount } from "svelte";
  import DialogHeader from "./DialogHeader.svelte";

  // `dirs`: every folder of the library (paths); `from`: the path being moved
  let { title = "移动到", nodePath = "", from, dirs, onok, onclose } = $props();
  let query = $state("");
  let busy = $state(false);
  let error = $state("");
  let box = $state();

  onMount(() => box?.focus());

  const here = $derived(from.includes("/") ? from.slice(0, from.lastIndexOf("/")) : "");
  // the top, and the folders it can go into: not itself, nothing inside itself, not where it is
  const places = $derived(
    [{ path: "", label: "（知识库顶层）" }, ...dirs.map((d) => ({ path: d, label: d.replaceAll("/", " / ") }))].filter(
      (p) => p.path !== from && !p.path.startsWith(from + "/") && p.path !== here && p.label.toLowerCase().includes(query.trim().toLowerCase()),
    ),
  );

  async function pick(p) {
    if (busy) return;
    busy = true;
    error = "";
    try {
      await onok(p.path);
      onclose();
    } catch (e) {
      error = String(e);
      busy = false;
    }
  }
</script>

<div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog dialog-shell" role="dialog" aria-modal="true" aria-label={title} tabindex="-1" onkeydown={(e) => e.key === "Escape" && onclose()}>
    <DialogHeader {title} path={nodePath} />
    <div class="dialog-content">
    <p class="muted from">要移到哪个文件夹？</p>
    <input type="text" bind:value={query} bind:this={box} placeholder="输入文字筛选文件夹" aria-label="筛选文件夹" />
    <ul>
      {#each places as p (p.path)}
        <li><button onclick={() => pick(p)} disabled={busy}>{p.label}</button></li>
      {:else}
        <li class="muted none">没有可以移动到的文件夹</li>
      {/each}
    </ul>
    {#if error}<p class="error">{error}</p>{/if}
    </div>
    <div class="actions"><button type="button" onclick={onclose}>取消</button></div>
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; z-index: 950; background: rgba(0, 0, 0, 0.3); display: flex; align-items: center; justify-content: center; }
  .dialog { width: min(480px, calc(100vw - 32px)); }
  .from { margin: 0 0 8px; font-size: 13px; }
  ul { list-style: none; margin: 8px 0 0; padding: 0; border: 1px solid var(--line); border-radius: 6px; }
  li button { width: 100%; text-align: left; border: none; background: none; padding: 6px 10px; border-radius: 0; }
  li button:hover { background: var(--accent-soft); }
  .none { padding: 10px; }
  .error { color: var(--hl); font-size: 13px; }
  .actions { display: flex; justify-content: flex-end; margin-top: 12px; }
</style>
