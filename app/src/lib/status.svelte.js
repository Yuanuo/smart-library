// The one-line status slot at the bottom of the directory tree (`App.svelte`
// `.side-foot`, left side). Any long-running task can put a short note here
// while it runs and take it away when it is done; with nothing to report the
// slot shows `status.idle`.
//
// Entries are keyed (`index:<lib>`, `export:<lib>`, …) so a task clears only
// its own line and several tasks can report at the same time.

const items = $state({});

export const status = {
  /** Shown when no task is reporting. */
  idle: "知识库",

  get text() {
    return Object.values(items).filter(Boolean).join("　");
  },

  /** Show `text` under `key`; an empty text removes the key. */
  set(key, text) {
    if (text) items[key] = text;
    else delete items[key];
  },

  clear(key) {
    delete items[key];
  },
};
