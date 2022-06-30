package org.appxi.smartlib.app.item;

import javafx.beans.property.SimpleStringProperty;
import javafx.beans.property.StringProperty;
import javafx.scene.input.DataFormat;
import org.appxi.smartlib.FolderProvider;
import org.appxi.smartlib.Item;
import org.appxi.smartlib.ItemProvider;

public class ItemEx extends Item {
    public static final DataFormat DND_ITEM = new DataFormat("application/x-item-serialized-object");

    public static final ItemEx ROOT = new ItemEx("ROOT", "", FolderProvider.ONE);

    public final StringProperty name, path;

    public ItemEx(ItemProvider provider) {
        this(null, null, provider);
    }

    public ItemEx(String name, String path, ItemProvider provider) {
        super(provider);
        this.name = new SimpleStringProperty(name);
        this.path = new SimpleStringProperty(path);
    }

    @Override
    public Item root() {
        return ROOT;
    }

    @Override
    public ItemEx setName(String name) {
        this.name.set(name);
        return this;
    }

    @Override
    public String getName() {
        return this.name.get();
    }

    @Override
    public ItemEx setPath(String path) {
        this.path.set(path);
        return this;
    }

    @Override
    public String getPath() {
        return this.path.get();
    }

    @Override
    public ItemEx clone(String name, String path, ItemProvider provider) {
        return new ItemEx(name, path, provider);
    }
}
