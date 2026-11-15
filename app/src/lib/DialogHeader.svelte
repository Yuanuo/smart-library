<script>
  let { title, path = "" } = $props();
</script>

<header class="dialog-header">
  <h2>{title}</h2>
  {#if path}
    <input
      type="text"
      class="dialog-path"
      readonly
      value={path}
      title={path}
      aria-label="节点路径"
      onclick={(e) => e.currentTarget.select()}
    />
  {/if}
</header>

<style>
  h2 { margin: 0; font-size: 18px; color: var(--accent); }
  .dialog-path {
    display: block;
    width: 100%;
    box-sizing: border-box;
    margin-top: 4px;
    padding: 0;
    border: none;
    background: transparent;
    color: var(--muted);
    font-size: 12px;
    line-height: 1.4;
    outline: none;
    text-overflow: ellipsis;
  }
</style>
