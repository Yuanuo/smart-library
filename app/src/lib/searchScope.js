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

export function scopesForLibrary(scopes, id) {
  const items = scopes.filter((item) => item.lib === id);
  return items.some((item) => item.whole) ? [] : items;
}

export function setLibrarySearchScopes(search, scopes) {
  search.scopes = [...search.scopes.filter((item) => item.lib !== search.lib), ...scopes];
  search.libraryPinned = true;
}

/** Shared by the scope pane and the library tree's context menu. */
export function addScopesToSearch(search, items) {
  if (!items.length) return false;
  search.tab = "scope";
  // The library tree's multi-selection belongs to one library.
  const id = items[0].lib;
  const current = scopesForLibrary(search.scopes, id);
  const scopes = new Map(current.map((item) => [scopeKey(item), item]));
  for (const item of items) {
    if (item.whole) {
      scopes.clear();
      break;
    }
    const key = scopeKey(item);
    if (!scopes.has(key)) scopes.set(key, item);
  }
  if (search.lib === id && search.libraryPinned && scopes.size === current.length) return false;
  if (search.lib !== id) search.filters = {};
  search.lib = id;
  setLibrarySearchScopes(search, [...scopes.values()]);
  return true;
}
