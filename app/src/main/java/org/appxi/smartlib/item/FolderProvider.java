package org.appxi.smartlib.item;

import javafx.scene.Node;
import javafx.scene.control.TreeItem;
import org.appxi.javafx.visual.MaterialIcon;
import org.appxi.smartlib.dao.DataApi;

import java.util.List;

public final class FolderProvider extends AbstractProvider {
    public static final FolderProvider ONE = new FolderProvider();

    private FolderProvider() {
    }

    @Override
    public boolean isDirectory() {
        return true;
    }

    @Override
    public String providerId() {
        return null;
    }

    @Override
    public String providerName() {
        return "目录";
    }

    @Override
    public Node getItemIcon(TreeItem<Item> treeItem) {
        if (null == treeItem) return MaterialIcon.CREATE_NEW_FOLDER.graphic();
        return treeItem.isExpanded() ? MaterialIcon.FOLDER_OPEN.graphic() : MaterialIcon.FOLDER.graphic();
    }

    public List<Item> getItemChildren(Item item) {
        return DataApi.dataAccess().list(item);
    }
}
