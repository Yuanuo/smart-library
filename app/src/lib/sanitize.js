const ALLOWED_TAGS = new Set([
  "a", "b", "blockquote", "br", "code", "dd", "del", "div", "dl", "dt",
  "em", "h1", "h2", "h3", "h4", "h5", "h6", "hr", "i", "ins", "kbd",
  "li", "mark", "ol", "p", "pre", "q", "rp", "rt", "ruby", "s", "samp",
  "small", "span", "strong", "sub", "sup", "table", "tbody", "td", "tfoot",
  "th", "thead", "tr", "u", "ul",
]);

const DROP_WITH_CONTENT = new Set([
  "script", "style", "iframe", "frame", "frameset", "object", "embed",
  "applet", "noscript", "template", "svg", "math",
]);

const ALLOWED_ATTRS = new Set(["class", "title", "data-dict", "data-dict-id"]);

function isSafeHref(val) {
  const s = val.trim().toLowerCase();
  if (!s || s.startsWith("#")) return true;
  if (s.startsWith("http://") || s.startsWith("https://")) return true;
  return false;
}

function cleanNode(node, doc) {
  if (node.nodeType === Node.TEXT_NODE) {
    return doc.createTextNode(node.textContent || "");
  }
  if (node.nodeType !== Node.ELEMENT_NODE) {
    return null;
  }
  const tag = node.tagName.toLowerCase();
  if (DROP_WITH_CONTENT.has(tag)) {
    return null;
  }
  const frag = doc.createDocumentFragment();
  if (!ALLOWED_TAGS.has(tag)) {
    for (const child of node.childNodes) {
      const cleaned = cleanNode(child, doc);
      if (cleaned) frag.appendChild(cleaned);
    }
    return frag;
  }
  const el = doc.createElement(tag);
  for (const attr of node.attributes) {
    const name = attr.name.toLowerCase();
    if (name === "href" && tag === "a") {
      if (isSafeHref(attr.value)) el.setAttribute("href", attr.value);
    } else if (ALLOWED_ATTRS.has(name)) {
      el.setAttribute(name, attr.value);
    }
  }
  for (const child of node.childNodes) {
    const cleaned = cleanNode(child, doc);
    if (cleaned) el.appendChild(cleaned);
  }
  return el;
}

/** Sanitize untrusted HTML (e.g. dictionary entries) against XSS before `{@html}`. */
export function sanitizeHtml(html) {
  if (!html) return "";
  if (typeof DOMParser === "undefined") return html;
  const parsed = new DOMParser().parseFromString(html, "text/html");
  const out = document.createElement("div");
  for (const child of parsed.body.childNodes) {
    const cleaned = cleanNode(child, document);
    if (cleaned) out.appendChild(cleaned);
  }
  return out.innerHTML;
}
