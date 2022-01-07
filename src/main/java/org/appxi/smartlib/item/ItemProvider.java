package org.appxi.smartlib.item;

import javafx.scene.Node;
import javafx.scene.control.TreeItem;
import org.appxi.search.solr.Piece;

import java.util.List;
import java.util.function.Consumer;
import java.util.function.Function;

public interface ItemProvider {
    String providerId();

    String providerName();

    boolean isDirectory();

    default Node getItemIcon(TreeItem<Item> treeItem) {
        return null;
    }

    default String getItemName(String name) {
        return name;
    }

    Consumer<Item> getCreator();

    Function<Item, ItemEditor> getEditor();

    Function<Item, ItemViewer> getViewer();

    Function<Item, List<Piece>> getIndexer();
}
