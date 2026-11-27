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

export function searchScope(library, node, label = library.title, depth) {
  const within = node ? withinOf(library, node) : null;
  return {
    lib: library.id, kind: library.kind, label, depth,
    books: within?.books ?? [], paths: within?.paths ?? [],
    rels: within?.rel ? [within.rel] : [], whole: !within,
  };
}

function covers(parent, child, noteChains) {
  if (parent.lib !== child.lib) return false;
  if (parent.whole) return true;
  if (child.whole) return false;
  const { books = [], paths = [], rels = [] } = child;
  if (!books.length && !paths.length && !rels.length) return parent.label === child.label;
  const withinRel = (id) => parent.rels?.some((rel) =>
    id === rel || id.startsWith(`${rel}/`) || noteChains?.get(parent.lib)?.get(id)?.includes(rel));
  return books.every((book) => parent.books?.includes(book))
    && rels.every(withinRel)
    && paths.every((path) => parent.paths?.includes(path) || withinRel(path));
}

export function mergeSearchScopes(current, items, noteChains) {
  let scopes = current;
  for (const item of items) {
    // A static tree parent can cover the same documents as its only child.
    const shallower = (existing) => (item.depth ?? Infinity) < (existing.depth ?? Infinity);
    if (scopes.some((existing) => covers(existing, item, noteChains) &&
      !(shallower(existing) && covers(item, existing, noteChains)))) continue;
    scopes = [...scopes.filter((existing) => !covers(item, existing, noteChains)), item];
  }
  return scopes;
}

// Notes use numeric IDs, so ancestry comes from the live tree, including for
// ranges restored from a workspace saved before source kinds were recorded.
export async function normalizeSearchScopes(scopes, noteChain) {
  const ids = new Map();
  for (const scope of scopes) {
    if (scope.whole || (scope.kind && scope.kind !== "notes")) continue;
    const rels = [...(scope.rels ?? []), ...(scope.paths ?? [])].filter((id) => /^\d+$/.test(id));
    if (!rels.length) continue;
    if (!ids.has(scope.lib)) ids.set(scope.lib, new Set());
    for (const id of rels) ids.get(scope.lib).add(id);
  }
  const chains = await Promise.all([...ids].filter(([, rels]) => rels.size > 1).map(async ([lib, rels]) => {
    const entries = await Promise.all([...rels].map(async (id) => [id, await noteChain(lib, id).catch(() => [])]));
    return [lib, new Map(entries)];
  }));
  return mergeSearchScopes([], scopes, new Map(chains));
}

/** Shared by the scope pane and the library tree's context menu. */
export function addScopesToSearch(search, items, followLibrary = false) {
  if (!items.length) return false;
  search.tab = "scope";
  const id = items[0].lib;
  const viewChanged = followLibrary && (search.lib !== id || !search.libraryPinned);
  if (viewChanged) {
    if (search.lib !== id) search.filters = {};
    search.lib = id;
    search.libraryPinned = true;
  }
  const scopes = mergeSearchScopes(search.scopes, items);
  const scopesChanged = scopes !== search.scopes;
  if (scopesChanged) search.scopes = scopes;
  return viewChanged || scopesChanged;
}
