import * as api from "./api.js";

/**
 * Build a reactive context-menu item for "查拼音" and asynchronously populate
 * its label with the pinyin of the first 5 characters so short selections are
 * visible directly in the menu without clicking.
 */
export function makePinyinItem(text, onOpen) {
  const chars = [...text];
  const previewSrc = chars.slice(0, 5).join("");
  const suffix = chars.length > 5 ? "…" : "";
  const item = $state({ label: "查拼音：…", action: onOpen });
  api.toPinyin(previewSrc)
    .then((py) => {
      item.label = `查拼音：${py}${suffix}`;
    })
    .catch(() => {
      item.label = `查拼音：${previewSrc}${suffix}`;
    });
  return item;
}
