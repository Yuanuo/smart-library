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
  {:else if name === "mindmap"}
    <circle cx="12" cy="12" r="2.6" fill="currentColor" fill-opacity="0.25" /><circle cx="4.5" cy="6" r="1.8" /><circle cx="4.5" cy="18" r="1.8" /><circle cx="19.5" cy="7" r="1.8" /><circle cx="19.5" cy="17" r="1.8" /><path d="M9.5 11 6 7M9.5 13 6 17M14.5 11l3.5-3M14.5 13l3.5 3" />
  {:else if name === "whiteboard"}
    <rect x="3.5" y="4.5" width="17" height="12" rx="1.5" /><path d="M8 20l2-3.5M16 20l-2-3.5M7 13l3-3 2.5 2 3.5-4" />
  {:else if name === "archive"}
    <!-- a data package (zip): a box with a lid -->
    <rect x="3.5" y="4.5" width="17" height="4.5" rx="1" fill="currentColor" fill-opacity="0.25" /><path d="M5 9v10a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1V9" /><path d="M10 13h4" />
  {:else if name === "folder-lock"}
    <!-- a local folder, read-only -->
    <path d="M3 6.5A1.5 1.5 0 0 1 4.5 5H9l2 2.5h8.5A1.5 1.5 0 0 1 21 9v9.5a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 18.5z" /><rect x="9.5" y="13" width="5" height="4.5" rx="0.8" fill="currentColor" fill-opacity="0.3" /><path d="M10.5 13v-1.2a1.5 1.5 0 0 1 3 0V13" />
  {:else if name === "folder-edit"}
    <!-- a local folder, read-write -->
    <path d="M3 6.5A1.5 1.5 0 0 1 4.5 5H9l2 2.5h8.5A1.5 1.5 0 0 1 21 9v9.5a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 18.5z" /><path d="m9.5 17.8.6-2.6 5.2-5.2 2 2-5.2 5.2z" fill="currentColor" fill-opacity="0.3" />
  {:else if name === "database"}
    <!-- an internal library (database) -->
    <ellipse cx="12" cy="6" rx="7" ry="2.8" fill="currentColor" fill-opacity="0.25" /><path d="M5 6v12c0 1.5 3.1 2.8 7 2.8s7-1.3 7-2.8V6" /><path d="M5 12c0 1.5 3.1 2.8 7 2.8s7-1.3 7-2.8" />
  {:else if name === "gear"}
    <path d="M12 3.5l1.4 2.2 2.5-.7.9 2.4 2.5.7-.4 2.6L21 12l-1.9 1.3.4 2.6-2.5.7-.9 2.4-2.5-.7L12 20.5l-1.4-2.2-2.5.7-.9-2.4-2.5-.7.4-2.6L3 12l1.9-1.3-.4-2.6 2.5-.7.9-2.4 2.5.7z" /><circle cx="12" cy="12" r="3" />
  {:else if name === "message"}
    <path d="M4 5h16v11H10l-4.5 4v-4H4z" /><path d="M8 9.5h8M8 12.5h5" />
  {:else if name === "speaker"}
    <path d="M4 9.5h3.5L12 5.5v13l-4.5-4H4z" fill="currentColor" fill-opacity="0.2" /><path d="M15.5 9a4 4 0 0 1 0 6M18 6.5a7.5 7.5 0 0 1 0 11" />
  {:else if name === "info"}
    <circle cx="12" cy="12" r="8.5" /><path d="M12 11v5.5" /><circle cx="12" cy="7.8" r="0.9" fill="currentColor" />
  {:else if name === "locate"}
    <circle cx="12" cy="12" r="6" /><circle cx="12" cy="12" r="1.5" fill="currentColor" /><path d="M12 2v4M12 18v4M2 12h4M18 12h4" />
  {:else if name === "sparkles"}
    <path d="M12 3l1.9 4.6L18.5 9.5l-4.6 1.9L12 16l-1.9-4.6L5.5 9.5l4.6-1.9z" fill="currentColor" fill-opacity="0.22" /><path d="M19 15l.9 2.1L22 18l-2.1.9L19 21l-.9-2.1L16 18l2.1-.9z" /><path d="M5 16l.7 1.6L7.3 18.3l-1.6.7L5 20.6l-.7-1.6-1.6-.7 1.6-.7z" />
  {:else if name === "dict"}
    <path d="M5 4.5A2 2 0 0 1 7 2.5h11.5v16H7a2 2 0 0 0-2 2z" fill="currentColor" fill-opacity="0.18" /><path d="M5 18.5v1A2 2 0 0 0 7 21.5h11.5" /><path d="M9 7h6M9 10.5h4" />
  {:else if name === "open-window"}
    <path d="M14 4h6v6" /><path d="M20 4 11 13" /><path d="M18 13v5.5a1.5 1.5 0 0 1-1.5 1.5h-11A1.5 1.5 0 0 1 4 18.5v-11A1.5 1.5 0 0 1 5.5 6H11" />
  {/if}
</svg>

<style>
  .icon-svg { flex: none; display: block; }
</style>
