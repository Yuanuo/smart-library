package org.appxi.smartlib.html;

abstract class Utils {
    private Utils() {
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
}
