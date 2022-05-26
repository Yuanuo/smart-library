package org.appxi.smartlib.html;

import org.appxi.util.NumberHelper;
import org.jsoup.nodes.Attributes;
import org.jsoup.nodes.Comment;
import org.jsoup.nodes.Element;
import org.jsoup.nodes.Node;
import org.jsoup.nodes.TextNode;
import org.jsoup.select.NodeVisitor;

import java.util.Arrays;
import java.util.HashSet;
import java.util.List;
import java.util.Set;

public class HtmlRepairer implements NodeVisitor {
    private static final String lineSplitter = "(([，、。？：；！]+)|([,.?:;!]\s+)|([།༎]\s+))";
    private static final List<String> messes = List.of("font-family: \"\";", "font-size: x-large;", "font-size: xx-large;");

    private final Set<String> idTags = new HashSet<>();
    private long edition;

    public HtmlRepairer(long edition) {
        this.edition = edition;
    }

    public long edition() {
        return edition;
    }

    public HtmlRepairer withMost() {
        return this.with("div", "p", "pre", "h1", "h2", "h3", "h4", "h5", "h6"
                , "a", "blockquote", "cite", "code", "dd", "dl", "dt", "em", "form", "header", "footer"
                , "img", "li", "ol", "q", "section", "summary", "table", "td", "th", "textarea", "ul");
    }

    public HtmlRepairer with(String... tagNames) {
        this.idTags.addAll(Arrays.asList(tagNames));
        return this;
    }

    @Override
    public void head(Node node, int depth) {
        if (depth == 0) return;
        if (node instanceof Comment) {
            node.remove();
            return;
        }
        if (node instanceof Element ele) {
            final String tagName = ele.tagName();
            if (tagName.contains(":")) {
                ele.remove();
                return;
            }
            if ("br".equals(tagName) || "hr".equals(tagName)) return;

            final Attributes eleAttrs = ele.attributes();
            if ("p".equals(tagName) && eleAttrs.hasKey("data-pb")) return;

            String styleStr = eleAttrs.get("style");
            if (eleAttrs.hasKey("style") && styleStr.isBlank()) eleAttrs.remove("style");
            else if (!styleStr.isEmpty()) {
                if (styleStr.contains("caret-color"))
                    styleStr = null;
                else {
                    StringBuilder buff = new StringBuilder(styleStr);
                    for (String mess : messes) {
                        int i = buff.indexOf(mess);
                        if (i == -1) continue;
                        buff.delete(i, i + mess.length());
                    }
                    styleStr = buff.toString();
                }
                if (null == styleStr || styleStr.isBlank()) eleAttrs.remove("style");
                else eleAttrs.put("style", styleStr);
            }

            switch (tagName) {
                case "span":
                    if (eleAttrs.isEmpty() || eleAttrs.size() == 1 && eleAttrs.hasKey("id")) {
                        ele.unwrap();
                        return;
                    }
                    break;
                case "div":
                    if (depth == 1) {
                        ele.tagName("p");
                    }
                    break;
                case "b":
                case "strong":
                    if (HtmlHelper.headingTags.contains(ele.parent().tagName())) {
                        ele.unwrap();
                        return;
                    }
                    break;
            }

            if (!eleAttrs.hasKey("id") && this.idTags.contains(tagName) || ele.id().startsWith("temp-"))
                ele.id(nextID(null));
        } else if (node instanceof TextNode text && !text.isBlank()) {
            if (text.parent() instanceof Element parent) {
                if (null == text.previousSibling()) text.text(text.text().stripLeading());
                if (null == text.nextSibling()) text.text(text.text().stripTrailing());
                if (text.isBlank() || HtmlHelper.headingTags.contains(parent.tagName())) return;

                final String[] lines = text.text().replaceAll(lineSplitter, "$1∷\n").split("∷\n");
                if (lines.length == 1) {
                    return;
                }
                final Element wrap = node.ownerDocument().createElement("wrap");
                final StringBuilder buff = new StringBuilder();
                for (int i = 0; i < lines.length; i++) {
                    if (i > 0) buff.append("<a id=\"".concat(nextID("a-")).concat("\"/>"));
                    buff.append(lines[i]);
                }
                wrap.html(buff.toString());
                node.replaceWith(wrap);
                wrap.unwrap();
            }
        }
    }

    private String nextID(String prefix) {
        return (null == prefix ? "a" : prefix).concat(NumberHelper.toRadix62(edition++));
    }

    @Override
    public void tail(Node node, int depth) {
    }
}
