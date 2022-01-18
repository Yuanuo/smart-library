package org.appxi.smartlib.item;

import javafx.scene.Node;
import javafx.scene.control.TreeItem;
import org.appxi.javafx.visual.MaterialIcon;

import java.util.function.Consumer;
import java.util.function.Function;

public final class FileProvider extends AbstractProvider {
    public static final FileProvider ONE = new FileProvider();

    private FileProvider() {
    }

    @Override
    public String providerId() {
        return null;
    }

    @Override
    public String providerName() {
        return "文件";
    }

    @Override
    public boolean isDirectory() {
        return false;
    }

    @Override
    public Node getItemIcon(TreeItem<Item> treeItem) {
        return MaterialIcon.BLOCK.graphic();
    }

    @Override
    public Consumer<Item> getCreator() {
        return null;
    }

    @Override
    public Function<Item, ItemEditor> getEditor() {
        return null;
    }

    @Override
    public Function<Item, ItemViewer> getViewer() {
        return null;
    }

    @Override
    public Consumer<Item> getToucher() {
        return null;
    }
}
