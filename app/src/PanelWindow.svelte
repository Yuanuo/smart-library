<script>
  // A dialog living in its own window (see lib/panels.js). It works on the
  // same data as the dialog; opening an entry is handed to the main window,
  // and it reloads when the main window says data changed or when it gets
  // the focus.
  import { onMount } from "svelte";
  import { emit, listen } from "@tauri-apps/api/event";
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import RecentDialog from "./lib/RecentDialog.svelte";
  import MarksDialog from "./lib/MarksDialog.svelte";

  let { kind } = $props();
  const win = getCurrentWebviewWindow();
  let doc = $state(null); // the document the main window shows
  let refresh = $state(0);

  onMount(() => {
    const offs = [
      listen("current-doc", (e) => (doc = e.payload)),
      listen("data-changed", () => refresh++),
      win.onFocusChanged((e) => e.payload && refresh++),
    ];
    emit("panel-ready");
    return () => offs.forEach((p) => p.then((f) => f()));
  });
</script>

{#if kind === "recent"}
  <RecentDialog embedded {refresh} onopen={(item) => emit("panel-open", { kind: "recent", item })} onclose={() => win.close()} />
{:else if kind === "marks"}
  <MarksDialog embedded {refresh} {doc} onopen={(item) => emit("panel-open", { kind: "marks", item })} onclose={() => win.close()} />
{/if}
