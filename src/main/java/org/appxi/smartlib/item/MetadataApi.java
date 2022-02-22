package org.appxi.smartlib.item;

import org.appxi.smartlib.search.Searchable;

import java.util.List;

public interface MetadataApi {
    Item item();

    String getMetadata(String key, String defaultValue);

    List<String> getMetadata(String key);

    void removeMetadata(String key);

    void addMetadata(String key, String value);

    void setMetadata(String key, String value);

    default Searchable getSearchable() {
        return this.item().attrOr(Searchable.class, () -> Searchable.of(getMetadata("searchable", Searchable.all.name())));
    }

    default void setSearchable(Searchable searchable) {
        this.item().attr(Searchable.class, searchable);
        this.setMetadata("searchable", searchable.name());
    }
}
