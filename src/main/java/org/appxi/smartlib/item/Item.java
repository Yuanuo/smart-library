package org.appxi.smartlib.item;

import javafx.beans.property.SimpleStringProperty;
import javafx.beans.property.StringProperty;
import org.appxi.util.ext.Attributes;

import java.util.Objects;

public class Item extends Attributes {
    public static final Item ROOT = new Item("ROOT", "", FolderProvider.ONE);

    public final StringProperty name, path;
    public final ItemProvider provider;

    public Item(ItemProvider provider) {
        this(null, null, provider);
    }

    public Item(String name, String path, ItemProvider provider) {
        this.name = new SimpleStringProperty(name);
        this.path = new SimpleStringProperty(path);
        this.provider = provider;
    }

    public Item setName(String name) {
        this.name.set(name);
        return this;
    }

    public String getName() {
        return this.name.get();
    }

    public Item setPath(String path) {
        this.path.set(path);
        return this;
    }

    public String getPath() {
        return this.path.get();
    }

    @Override
    public String toString() {
        return this.name.get();
    }

    @Override
    public boolean equals(Object o) {
        if (this == o) return true;
        if (o == null || getClass() != o.getClass()) return false;
        Item item = (Item) o;
        return Objects.equals(getName(), item.getName()) && Objects.equals(getPath(), item.getPath());
    }

    @Override
    public int hashCode() {
        return Objects.hash(getName(), getPath());
    }

    @Override
    public Item clone() {
        return new Item(this.getName(), this.getPath(), this.provider);
    }

    public final boolean isRoot() {
        return this == ROOT || ROOT.getPath().equals(this.getPath());
    }

    public final String typedPath() {
        return "【".concat(provider.providerName()).concat("】: ").concat(this.path.get());
    }

    public final String parentPath() {
        String path = this.path.get();
        int idx = path.lastIndexOf('/');
        return idx == -1 ? "" : path.substring(0, idx);
    }

    public final Item parent() {
        String path = this.parentPath();
        if (path.isBlank())
            return ROOT;

        int idx = path.lastIndexOf('/');
        return new Item(idx == -1 ? path : path.substring(idx + 1), path, FolderProvider.ONE);
    }
}
