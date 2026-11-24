<script>
  // A short note about the program: version (YY.MM.DD) and what it does,
  // in words a reader (not a developer) understands.
  import * as api from "./api.js";

  let { info, onclose, oncheckupdate } = $props();

  const REPO_URL = "https://github.com/Yuanuo/smart-library";

  function openRepo() {
    api.openExternalUrl(REPO_URL).catch(() => {});
  }
</script>

<div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog dialog-shell" role="dialog" aria-modal="true" aria-label="关于" tabindex="-1"
    onkeydown={(e) => e.key === "Escape" && onclose()}>
    <header class="dialog-header"><h2>{info?.name ?? "Smart Library"}</h2></header>
    <div class="dialog-content">
      <p class="lead">一个放在你自己电脑上的书库：读书、查词、找出处，都在一个窗口里。轻便极速。</p>
      <ul class="points">
        <li><b>收集</b>：加入离线文库，也可以链接电脑上的文件夹，或者写自己的笔记。</li>
        <li><b>查找</b>：输入一句话、一个词或编号，立刻看到它出现在哪一部、哪一卷。</li>
        <li><b>阅读</b>：接近纸书的排版，注释、词典、跨书链接都在手边。</li>
        <li><b>保管</b>：内容存放在你自己的电脑上，随时可以备份或删除。</li>
      </ul>
    </div>
    <div class="actions">
      <button type="button" class="repo-btn" onclick={openRepo} title={REPO_URL}>
        <svg viewBox="0 0 24 24" width="15" height="15" fill="currentColor" aria-hidden="true">
          <path d="M12 .5a11.5 11.5 0 0 0-3.64 22.41c.58.11.79-.25.79-.56v-2c-3.2.7-3.88-1.54-3.88-1.54-.52-1.33-1.28-1.69-1.28-1.69-1.05-.71.08-.7.08-.7 1.16.08 1.77 1.19 1.77 1.19 1.03 1.76 2.7 1.25 3.36.96.1-.75.4-1.25.73-1.54-2.55-.29-5.24-1.28-5.24-5.68 0-1.26.45-2.28 1.19-3.09-.12-.29-.52-1.46.11-3.05 0 0 .97-.31 3.17 1.18a11 11 0 0 1 5.77 0c2.2-1.49 3.17-1.18 3.17-1.18.63 1.59.23 2.76.11 3.05.74.81 1.19 1.83 1.19 3.09 0 4.41-2.69 5.39-5.26 5.67.41.36.78 1.07.78 2.15v3.19c0 .31.21.67.8.56A11.5 11.5 0 0 0 12 .5Z" />
        </svg>
        <span>项目主页</span>
      </button>
      <span class="grow"></span>
      <span class="ver">版本 {info?.version ?? ""}</span>
      <button onclick={oncheckupdate}>检查更新</button><button class="primary" onclick={onclose}>关闭</button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed; inset: 0; z-index: 950; background: rgba(0, 0, 0, 0.3);
    display: flex; align-items: center; justify-content: center;
  }
  .dialog { width: min(600px, calc(100vw - 32px)); }
  h2 { margin: 0 0 4px; font-size: 22px; color: var(--accent); }
  .lead { margin: 0 0 14px; font-size: 15px; line-height: 1.7; }
  .points { margin: 0; padding: 0; list-style: none; display: grid; gap: 8px; }
  .points li { line-height: 1.7; padding-left: 2px; }
  .points b { color: var(--accent); font-weight: 600; margin-right: 2px; }
  .actions { display: flex; align-items: center; gap: 8px; }
  .repo-btn { display: inline-flex; align-items: center; gap: 6px; }
  .grow { flex: 1; }
  .ver { color: var(--muted); white-space: nowrap; }
</style>
