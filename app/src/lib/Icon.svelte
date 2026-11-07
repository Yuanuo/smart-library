<script>
  // Small inline icons (no icon font): library, folder (closed / open), file,
  // chevrons for the tree, quick search (paper plane), search, plus, locate.
  let { name, size = 16 } = $props();
</script>

<svg viewBox="0 0 24 24" width={size} height={size} fill="none" stroke="currentColor" stroke-width="1.8"
  stroke-linejoin="round" stroke-linecap="round" aria-hidden="true" class="icon-svg">
  {#if name === "library"}
    <path d="M4 20V5.5L12 3l8 2.5V20" /><path d="M4 20h16" /><path d="M9 20v-6h6v6" /><path d="M9 9h6" />
  {:else if name === "folder"}
    <path d="M3 6.5A1.5 1.5 0 0 1 4.5 5H9l2 2.5h8.5A1.5 1.5 0 0 1 21 9v9.5a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 18.5z" fill="currentColor" fill-opacity="0.25" />
  {:else if name === "folder-open"}
    <path d="M3 18.5V6.5A1.5 1.5 0 0 1 4.5 5H9l2 2.5h7A1.5 1.5 0 0 1 19.5 9v2" /><path d="M3 18.5 5.5 11.5h16.2a1 1 0 0 1 .95 1.3L20.5 18.5a1.5 1.5 0 0 1-1.4 1H4.5A1.5 1.5 0 0 1 3 18.5z" fill="currentColor" fill-opacity="0.25" />
  {:else if name === "file"}
    <path d="M6 3h8l4 4v13.5a.5.5 0 0 1-.5.5h-11a.5.5 0 0 1-.5-.5V3.5A.5.5 0 0 1 6.5 3z" fill="currentColor" fill-opacity="0.2" /><path d="M14 3v4h4" /><path d="M9 12h6M9 15.5h6" />
  {:else if name === "chevron-right"}
    <path d="m9 5 7 7-7 7" stroke-width="2.4" />
  {:else if name === "chevron-down"}
    <path d="m5 9 7 7 7-7" stroke-width="2.4" />
  {:else if name === "plus"}
    <path d="M12 5v14M5 12h14" stroke-width="2.2" />
  {:else if name === "plane"}
    <path d="M21 3 3 10.5l7 2.5 2.5 7z" /><path d="M21 3 10 13" />
  {:else if name === "search"}
    <circle cx="10.5" cy="10.5" r="6.5" /><path d="m15.5 15.5 5 5" />
  {:else if name === "bookmark"}
    <path d="M6.5 3.5h11v17l-5.5-4-5.5 4z" fill="currentColor" fill-opacity="0.2" />
  {:else if name === "tag"}
    <path d="M3.5 12.2V4.5a1 1 0 0 1 1-1h7.7a1 1 0 0 1 .7.3l7.6 7.6a1 1 0 0 1 0 1.4l-7.7 7.7a1 1 0 0 1-1.4 0l-7.6-7.6a1 1 0 0 1-.3-.7z" /><circle cx="8.3" cy="8.3" r="1.4" fill="currentColor" />
  {:else if name === "clock"}
    <circle cx="12" cy="12" r="8.5" /><path d="M12 7v5.5l3.5 2" />
  {:else if name === "locate"}
    <circle cx="12" cy="12" r="6" /><circle cx="12" cy="12" r="1.5" fill="currentColor" /><path d="M12 2v4M12 18v4M2 12h4M18 12h4" />
  {/if}
</svg>

<style>
  .icon-svg { flex: none; display: block; }
</style>
