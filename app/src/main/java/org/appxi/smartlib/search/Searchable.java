package org.appxi.smartlib.search;

public enum Searchable {
    all("全部范围"),
    lookup("仅快捷检索"),
    exclude("不可搜索");

    public final String title;

    Searchable(String title) {
        this.title = title;
    }

    public static Searchable of(String name) {
        for (Searchable itm : values()) {
            if (itm.name().equals(name)) return itm;
        }
        return all;
    }

    @Override
    public String toString() {
        return this.title;
    }
}
