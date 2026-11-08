// The full path of a document, for "copy path".

/** Where a document is: a file path for folders, `<package>!/<path>` inside a
 * data package; notes have no file, so their title chain is used by the caller. */
export function fullPath(l, path) {
  if (!l) return path || "";
  if (!path) return l.source || l.title;
  if (l.kind === "folder") {
    const sep = l.source.includes("\\") ? "\\" : "/";
    return l.source.replace(/[\\/]+$/, "") + sep + path.split("/").join(sep);
  }
  return `${l.source}!/${path}`;
}
