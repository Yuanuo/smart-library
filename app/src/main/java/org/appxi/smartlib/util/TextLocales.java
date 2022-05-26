package org.appxi.smartlib.util;

import org.appxi.util.ChineseHelper;

import java.util.function.Predicate;

public abstract class TextLocales {
    private TextLocales() {
    }

    public static String extractEnglishText(StringBuilder text) {
        // 1. extract english by unicode range
        return extract(text, false, v -> v >= 0x0000 && v <= 0x02FF);
    }

    public static String extractLeftoversText(StringBuilder text) {
        // 2. extract english by all leftovers
        return text.toString();// extract(text, false, v -> true)
    }

    public static String extractTibetanText(StringBuilder text) {
        //or Character.UnicodeScript.of(v) == UnicodeScript.TIBETAN
        return extract(text, true, v -> v >= 0x0F00 && v <= 0x0FFF);
    }

    public static final Predicate<Integer> asciiTextFilter = v -> v >= 0x0000 && v <= 0x00FF;
    public static final Predicate<Integer> latinTextFilter = v -> v >= 0x0000 && v <= 0x02FF;
    public static final Predicate<Integer> tibetanTextFilter = v -> v >= 0x0F00 && v <= 0x0FFF;
    public static final Predicate<String> tibetanTextFinder = s -> s != null && s.matches(".*[\u0f00-\u0fff].*");

    public static String extractCJKbiGramText(StringBuilder text) {
        return extract(text, true, v -> Character.isIdeographic(v) || ChineseHelper.isChinesePunctuation(v));
    }

    public static String extractChineseText(StringBuilder text) {
        return extract(text, true, v ->
                Character.UnicodeScript.of(v) == Character.UnicodeScript.HAN || ChineseHelper.isChinesePunctuation(v));
    }

    public static String extract(final StringBuilder text, final boolean cleanExtracted, Predicate<Integer> nonLatinSelector) {
        final StringBuilder buff = new StringBuilder();
        for (int i = 0; i < text.length(); i++) {
            int codePoint = text.codePointAt(i);
            // always keep
            if (codePoint >= 0x0000 && codePoint <= 0x02FF) {//codePoint == 0x20 || '\r' == (char) codePoint || '\n' == (char) codePoint
                buff.appendCodePoint(codePoint);
                continue;
            }
            if (null != nonLatinSelector && nonLatinSelector.test(codePoint)) {
                buff.appendCodePoint(codePoint);
                if (cleanExtracted)
                    text.deleteCharAt(i--);
            }
        }
        return buff.toString();
    }

    public static void extractAndRemove(final StringBuilder text, Predicate<Integer> codePointFilter) {
        for (int i = 0; i < text.length(); i++) {
            if (codePointFilter.test(text.codePointAt(i))) {
                text.deleteCharAt(i--);
            }
        }
    }

    public static String extractAndCollect(String text, Predicate<Integer> codePointFilter) {
        final StringBuilder buff = new StringBuilder();
        int codePoint;
        for (int i = 0; i < text.length(); i++) {
            if (codePointFilter.test(codePoint = text.codePointAt(i))) {
                buff.appendCodePoint(codePoint);
            }
        }
        return buff.toString();
    }
}
