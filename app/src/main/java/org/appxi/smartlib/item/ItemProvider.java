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

    default Function<Item, ItemRenderer> getEditor() {
        return null;
    }

    default Function<Item, ItemRenderer> getViewer() {
        return null;
    }

    Function<Item, List<Piece>> getIndexer();

    default Consumer<Item> getToucher() {
        return null;
    }
}
