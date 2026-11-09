// Read-write mode of a folder library (design-spec 6.5): the user has to
// accept these rules before the program may change files in the folder.
import { ask } from "@tauri-apps/plugin-dialog";

export const WRITABLE_RULES = [
  "开启读写后，本程序会在这个文件夹里新建、修改、重命名和删除文件，包括：",
  "　· Markdown 笔记（.md）",
  "　· 脑图（.map.json）和白板（.wbd.json）",
  "其他类型的文件仍然只读，不会被改动。",
  "",
  "请注意：",
  "· Markdown 经排版编辑器保存后，格式可能被规范化（例如列表符号、空行、表格写法），与别的软件写法不同的地方可能改变；",
  "· 删除的文件会移到文件夹里的 .smart-library/trash，不会立刻彻底删除；",
  "· 文件名必须符合「本地文件名限制」（不能含 \\ / : * ? \" < > |，不能用系统保留名等）；",
  "· 如果文件夹同时被别的软件修改，保存时发现文件已变化会拒绝覆盖；",
  "· 建议先用 git 或其他方式备份这个文件夹。",
].join("\n");

/** Ask the user to accept the rules; true when they do. */
export function confirmWritable(title) {
  return ask(`把「${title}」设为读写模式？\n\n${WRITABLE_RULES}`, {
    title: "开启读写模式",
    kind: "warning",
    okLabel: "我了解，开启读写",
    cancelLabel: "取消",
  });
}

/** A file name has a suffix the program edits (what appears as a note in a read-write folder). */
export const isEditableName = (name) => /.\.(md|map\.json|wbd\.json)$/i.test(name ?? "");
