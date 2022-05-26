package org.appxi.smartlib.html;

import org.appxi.util.DigestHelper;
import org.jsoup.nodes.Element;
import org.jsoup.select.Elements;

import java.net.URLDecoder;
import java.net.URLEncoder;
import java.nio.charset.StandardCharsets;
import java.util.HashMap;
import java.util.Map;
import java.util.Set;

public abstract class HtmlHelper {
    public static final String headings = "h1,h2,h3,h4,h5,h6";
    public static final Set<String> headingTags = Set.of(headings.split(","));

    private HtmlHelper() {
    }

    public static String escapeJavaStyleString(String str, boolean escapeSingleQuote, boolean escapeForwardSlash) {
        if (str == null) return null;
        StringBuilder out = new StringBuilder();
        for (int i = 0; i < str.length(); i++) {
            char ch = str.charAt(i);

            // handle unicode
            if (ch > 0xfff) {
                out.append("\\u").append(Integer.toHexString(ch));
            } else if (ch > 0xff) {
                out.append("\\u0").append(Integer.toHexString(ch));
            } else if (ch > 0x7f) {
                out.append("\\u00").append(Integer.toHexString(ch));
            } else if (ch < 32) {
                switch (ch) {
                    case '\b':
                        out.append('\\');
                        out.append('b');
                        break;
                    case '\n':
                        out.append('\\');
                        out.append('n');
                        break;
                    case '\t':
                        out.append('\\');
                        out.append('t');
                        break;
                    case '\f':
                        out.append('\\');
                        out.append('f');
                        break;
                    case '\r':
                        out.append('\\');
                        out.append('r');
                        break;
                    default:
                        if (ch > 0xf) {
                            out.append("\\u00").append(Integer.toHexString(ch));
                        } else {
                            out.append("\\u000").append(Integer.toHexString(ch));
                        }
                        break;
                }
            } else {
                switch (ch) {
                    case '\'' -> {
                        if (escapeSingleQuote) {
                            out.append('\\');
                        }
                        out.append('\'');
                    }
                    case '"' -> {
                        out.append('\\');
                        out.append('"');
                    }
                    case '\\' -> {
                        out.append('\\');
                        out.append('\\');
                    }
                    case '/' -> {
                        if (escapeForwardSlash) {
                            out.append('\\');
                        }
                        out.append('/');
                    }
                    default -> out.append(ch);
                }
            }
        }
        return out.toString();
    }

    public static String encodeURI(String str) {
        return URLEncoder.encode(str, StandardCharsets.UTF_8);
    }

    public static String decodeURI(String str) {
        return URLDecoder.decode(str, StandardCharsets.UTF_8);
    }

    public static void inlineFootnotes(Element element) {
        element.select("sup:matches(\\d+)").forEach(ele -> {
            if (!ele.parents().is("a")) ele.tagName("a");
        });

        Elements footnotes = element.select("p > a:first-child:matches(\\[\\d+\\])");
        Map<String, String> footnotesMap = new HashMap<>(footnotes.size());

        for (int i = footnotes.size() - 1; i >= 0; i--) {
            Element ele = footnotes.get(i);
            String key = ele.text().replaceAll("[\\[\\]]+", "").strip();
            if (footnotesMap.containsKey(key)) continue;

            Element elePrt = ele.parent();
            elePrt.remove();
            ele.remove();
            footnotesMap.put(key, elePrt.text().strip());
        }
        if (footnotesMap.isEmpty()) return;
        //
        element.select("a:matches(\\[\\d+\\])").forEach(ele -> {
            String ref = ele.attr("data-note");
            if (ref.startsWith("@")) ref = ref.substring(1).strip();
            else ref = ele.text().replaceAll("[\\[\\]]+", "").strip();
            //
            String val = footnotesMap.remove(ref);
            if (null != val) {
                ele.attr("data-note", HtmlHelper.encodeURI(val));
                ele.id("temp-".concat(DigestHelper.uid()));
                ele.empty();
            }
        });
        //
        if (footnotesMap.isEmpty()) {
            Element ele = element.select("> div > hr").last();
            if (null != ele) {
                ele = ele.parent();
                if (null != ele && ele == ele.parent().children().last())
                    ele.remove();
            }

            ele = element.select("> hr").last();
            if (null != ele && ele == ele.parent().children().last())
                ele.remove();
        }
    }
}
