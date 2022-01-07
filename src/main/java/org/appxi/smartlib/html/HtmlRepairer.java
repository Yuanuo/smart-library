package org.appxi.smartlib.html;

import org.appxi.util.DigestHelper;
import org.jsoup.nodes.Attributes;
import org.jsoup.nodes.Element;
import org.jsoup.nodes.Node;
import org.jsoup.nodes.TextNode;
import org.jsoup.select.NodeVisitor;

import java.util.Arrays;
import java.util.HashSet;
import java.util.List;
import java.util.Set;

public class HtmlRepairer implements NodeVisitor {
    public static final String lineSplitter = "([，、。？：；！,.?:;!།༎]+)";
    public static final List<String> messes = List.of("font-family: \"\";", "font-size: x-large;");

    private final Set<String> tagNames = new HashSet<>();

    public HtmlRepairer withMost() {
        return this.with("div", "p", "pre", "h1", "h2", "h3", "h4", "h5", "h6"
                , "a", "blockquote", "cite", "code", "dd", "dl", "dt", "em", "form", "header", "footer"
                , "img", "li", "ol", "q", "section", "summary", "table", "td", "th", "textarea", "ul");
    }

    public HtmlRepairer with(String... tagNames) {
        this.tagNames.addAll(Arrays.asList(tagNames));
        return this;
    }

    @Override
    public void head(Node node, int depth) {
        if (node instanceof Element ele) {
            String styleStr = ele.attr("style");
            if (!styleStr.isEmpty()) {
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
                if (null == styleStr || styleStr.isBlank()) ele.removeAttr("style");
                else ele.attr("style", styleStr);
            }

            final Attributes eleAttrs = ele.attributes();
            if ("span".equals(ele.tagName()) && ele.childNodeSize() > 0
                && (eleAttrs.size() == 0 || eleAttrs.size() == 1 && eleAttrs.hasKey("id"))
                && ele.childNodeSize() == ele.childrenSize()) {
                Element wrap = ele.ownerDocument().createElement("wrap");
                wrap.appendChildren(ele.children());
                ele.replaceWith(wrap);
                wrap.unwrap();
                return;
            }

            if (!eleAttrs.hasKey("id") && this.tagNames.contains(ele.tagName())) ele.id(DigestHelper.uid());
        } else if (node instanceof TextNode text && !text.isBlank()) {
            if (text.parent() instanceof Element parent) {
                if (parent.is("span[id]")) return;
                final String[] lines = text.text().replaceAll(lineSplitter, "$1∷\n").split("∷\n");
                if (lines.length == 1) {
                    if (!parent.hasAttr("id")) parent.id(DigestHelper.uid());
                    return;
                }
                final Element wrap = node.ownerDocument().createElement("wrap");
                for (String line : lines) wrap.appendElement("span").id(DigestHelper.uid()).text(line);
                node.replaceWith(wrap);
                wrap.unwrap();
            }
        }
    }

    @Override
    public void tail(Node node, int depth) {
    }
}
