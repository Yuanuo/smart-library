<script>
  // The content of one reading tab: one volume (a tab is identified by the
  // volume path). `p` is a reactive object owned by the workspace, updated
  // when the same volume is opened again (new target / query).
  import * as api from "./api.js";
  import Reader from "./Reader.svelte";

  let { p, onopen, onprogress, ontitle } = $props();

  let vol = $state(null);
  let error = $state("");
  let localTarget = $state(null);
  let localNonce = $state(0);

  // (re)load when the query of the tab changes; the path never changes
  $effect(() => {
    const { bookId, path, query, phrase } = p;
    api
      .openVolume(bookId, path, query || null, !!phrase)
      .then((v) => {
        vol = v;
        error = "";
        ontitle?.(`${v.title} ${v.label}`);
      })
      .catch((e) => (error = String(e)));
  });

  // a jump requested from outside (search hit, tree, restore) wins over a
  // chapter jump inside the tab
  $effect(() => {
    localTarget = p.target ?? (p.query && vol ? vol.first_hit_lb : null);
    localNonce = p.nonce ?? 0;
  });

  function chapter(c) {
    if (c.path === p.path) {
      localTarget = c.lb;
      localNonce += 1;
      p.quiet = false;
    } else {
      onopen({ bookId: p.bookId, path: c.path, lb: c.lb });
    }
  }
</script>

{#if error}
  <p class="error">{error}</p>
{:else if vol}
  <Reader
    {vol}
    target={localTarget}
    nonce={localNonce}
    quiet={!!p.quiet}
    bind:mode={p.mode}
    onvolume={(path) => onopen({ bookId: p.bookId, path })}
    onchapter={chapter}
    onprogress={(lb) => onprogress(lb, p.mode)} />
{:else}
  <p class="loading muted">载入中…</p>
{/if}

<style>
  .error { padding: 24px; color: var(--hl); }
  .loading { padding: 24px; }
</style>
