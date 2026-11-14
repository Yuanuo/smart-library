<script>
  import { onMount } from "svelte";
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import * as api from "./lib/api.js";
  import DictReaderView from "./lib/DictReaderView.svelte";

  let { info = {} } = $props();
  let detail = $state(null);
  let loading = $state(true);
  let error = $state("");

  onMount(async () => {
    try {
      const d = await api.dictEntry(info.dictId, info.entryId);
      detail = d;
      getCurrentWebviewWindow()
        .setTitle(`${d.title}（${d.dict_name}）— Smart Library`)
        .catch(() => {});
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  });
</script>

<div class="entry-win">
  {#if error}
    <div class="err">{error}</div>
  {:else}
    <DictReaderView {detail} {loading} showPopout={false} />
  {/if}
</div>

<style>
  .entry-win {
    height: 100vh;
    width: 100vw;
    display: flex;
    flex-direction: column;
    background: var(--paper);
    color: var(--ink);
    overflow: hidden;
  }
  .err {
    padding: 24px;
    color: var(--hl);
  }
</style>
