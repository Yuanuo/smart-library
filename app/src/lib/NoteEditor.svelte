<script>
  // The visual (WYSIWYG) Markdown editor of a note: Milkdown / Crepe. The
  // note stays Markdown; `onchange` gets the text after every edit. Pictures
  // pasted, dropped or chosen are kept by `onpicture(file)` (-> name) and
  // written as `![](attach:<name>)`. `value` is read once: to change it from
  // outside, make the editor again.
  import { onMount, onDestroy, untrack } from "svelte";
  import { Crepe } from "@milkdown/crepe";
  import { insert } from "@milkdown/kit/utils";
  import "@milkdown/crepe/theme/common/style.css";
  import "@milkdown/crepe/theme/frame.css";
  import * as api from "./api.js";

  let { value, lib, onchange, onpicture } = $props();

  let root = $state();
  let crepe = null;
  let gone = false;
  const initial = untrack(() => value);

  let prefix = "";
  async function assetPrefix() {
    if (!prefix) prefix = (await api.appInfo()).asset_prefix;
    return prefix;
  }

  /** `attach:<name>` is shown from the library's attachments. */
  async function proxy(url) {
    if (!url.startsWith("attach:")) return url;
    return `${await assetPrefix()}${lib}/attachments/${url.slice(7)}`;
  }

  const upload = async (file) => `attach:${await onpicture(file)}`;

  onMount(async () => {
    const F = Crepe.Feature;
    const c = new Crepe({
      root,
      defaultValue: initial,
      // what the notes reader can show: no tables, formulas, code editor
      features: { [F.Table]: false, [F.Latex]: false, [F.CodeMirror]: false, [F.TopBar]: false, [F.AI]: false },
      featureConfigs: {
        [F.ImageBlock]: { onUpload: upload, blockOnUpload: upload, inlineOnUpload: upload, proxyDomURL: proxy },
        [F.Placeholder]: { text: "写点什么……（输入 / 选择标题、列表、引用、图片；可以直接粘贴图片）" },
      },
    });
    c.on((l) => l.markdownUpdated((_, md) => !gone && onchange(md)));
    await c.create();
    if (gone) {
      c.destroy();
      return;
    }
    crepe = c;
  });

  onDestroy(() => {
    gone = true;
    crepe?.destroy();
  });

  /** A picture on the clipboard: keep it and put it where the cursor is. */
  async function paste(e) {
    const files = [...(e.clipboardData?.files ?? [])].filter((f) => f.type.startsWith("image/"));
    if (!files.length || !crepe) return;
    e.preventDefault();
    e.stopPropagation();
    for (const f of files) {
      try {
        const name = await onpicture(f);
        crepe.editor.action(insert(`![](attach:${name})`));
      } catch (err) {
        console.warn("picture", err);
      }
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="note-editor" bind:this={root} onpastecapture={paste}></div>

<style>
  .note-editor { height: 100%; overflow: auto; background: var(--paper); }
  /* the editor takes the colors and the reading font of the program */
  .note-editor :global(.milkdown) {
    --crepe-color-background: var(--paper);
    --crepe-color-on-background: var(--ink);
    --crepe-color-surface: var(--panel);
    --crepe-color-surface-low: var(--bg);
    --crepe-color-on-surface: var(--ink);
    --crepe-color-on-surface-variant: var(--muted);
    --crepe-color-outline: var(--line);
    --crepe-color-primary: var(--accent);
    --crepe-color-secondary: var(--accent-soft);
    --crepe-color-on-secondary: var(--ink);
    --crepe-color-hover: var(--accent-soft);
    --crepe-color-selected: var(--accent-soft);
    --crepe-color-inline-area: var(--bg);
    --crepe-font-default: var(--reader-font, inherit);
    --crepe-font-title: var(--reader-font, inherit);
    min-height: 100%;
  }
  .note-editor :global(.milkdown .ProseMirror) { padding: 24px 56px 80px; font-size: var(--reader-size, 19px); line-height: var(--reader-line, 1.9); }
</style>
