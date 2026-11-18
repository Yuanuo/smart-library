const ALLOWED_TAGS = new Set([
  "a", "b", "big", "blockquote", "br", "caption", "code", "dd", "del", "div", "dl", "dt",
  "em", "font", "h1", "h2", "h3", "h4", "h5", "h6", "hr", "i", "img", "ins", "kbd",
  "li", "mark", "ol", "p", "pre", "q", "rp", "rt", "ruby", "s", "samp",
  "small", "span", "strong", "sub", "sup", "table", "tbody", "td", "tfoot",
  "th", "thead", "tr", "u", "ul",
]);

const DROP_WITH_CONTENT = new Set([
  "script", "style", "iframe", "frame", "frameset", "object", "embed",
  "applet", "noscript", "template", "svg", "math",
]);

const ALLOWED_ATTRS = new Set(["class", "title", "data-dict", "data-dict-id"]);

const DATA_IMAGE_PREFIXES = [
  "data:image/png;base64,",
  "data:image/jpeg;base64,",
  "data:image/jpg;base64,",
  "data:image/gif;base64,",
  "data:image/bmp;base64,",
];

function isSafeHref(val) {
  const s = val.trim().toLowerCase();
  if (!s || s.startsWith("#")) return true;
  if (s.startsWith("http://") || s.startsWith("https://")) return true;
  return false;
}

/** Dictionary pictures come from `.words_extra` as base64 data URIs. */
function isSafeImageSrc(val) {
  const v = val.trim().toLowerCase();
  const prefix = DATA_IMAGE_PREFIXES.find((p) => v.startsWith(p));
  if (!prefix) return false;
  const data = v.slice(prefix.length);
  return data.length > 0 && /^[a-z0-9+/=]+$/.test(data);
}

function isSafeValue(val) {
  return !val.includes(":") && !val.includes("(");
}

function isSafeAttr(tag, name, val) {
  if (ALLOWED_ATTRS.has(name)) return true;
  if (tag === "a" && name === "href") return isSafeHref(val);
  if (tag === "img" && name === "src") return isSafeImageSrc(val);
  if (tag === "img" && ["alt", "width", "height"].includes(name)) return true;
  if (tag === "font" && ["color", "size"].includes(name)) return isSafeValue(val);
  if (["td", "th", "tr", "table", "p", "div"].includes(tag) && ["colspan", "rowspan", "align", "valign"].includes(name)) {
    return isSafeValue(val);
  }
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
    if (isSafeAttr(tag, name, attr.value)) {
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
