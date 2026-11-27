/** A node range for a whole subtree in the index. */
export function withinOf(library, node) {
  const books = new Set();
  const paths = new Set();
  if (library.kind === "folder" || library.kind === "notes") {
    return { lib: library.id, books: [], paths: [], rel: node.rel };
  }
  function collect(item) {
    if (item.book_id) books.add(item.book_id);
    else if (item.path && library.kind === "html") paths.add(item.path);
    item.children.forEach(collect);
  }
  collect(node);
  return { lib: library.id, books: [...books], paths: [...paths], rel: null };
}

export function searchScope(library, node, label = library.title) {
  const within = node ? withinOf(library, node) : null;
  return { lib: library.id, label, books: within?.books ?? [], paths: within?.paths ?? [], rels: within?.rel ? [within.rel] : [], whole: !within };
}

function scopeKey(scope) {
  return JSON.stringify([scope.lib, !!scope.whole, ...[scope.books, scope.paths, scope.rels].map((parts) => [...(parts ?? [])].sort())]);
}

export function addSearchScopeItems(current, items) {
  const scopes = new Map(current.map((item) => [scopeKey(item), item]));
  for (const item of items) {
    const key = scopeKey(item);
    if (!scopes.has(key)) scopes.set(key, item);
  }
  return [...scopes.values()];
}
