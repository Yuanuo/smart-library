package org.appxi.smartlib.item;

import javafx.scene.Node;
import javafx.scene.control.TreeItem;
import org.appxi.javafx.visual.MaterialIcon;
import org.appxi.smartlib.dao.DataApi;

import java.util.List;
import java.util.function.Function;

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

    @Override
    public Function<Item, ItemEditor> getEditor() {
        return null;
    }

    @Override
    public Function<Item, ItemViewer> getViewer() {
        return null;
    }

//    private BiConsumer<Item, LibraryTreeView> itemActionForTouch;
//    @Override
//    public BiConsumer<Item, LibraryTreeView> getItemActionForTouch() {
//        if (null != this.itemActionForTouch)
//            return this.itemActionForTouch;
//
//        return this.itemActionForTouch = (item, treeView) -> {
//            final Alert alert = new Alert(Alert.AlertType.CONFIRMATION);
//            alert.setHeaderText("接触");
//            alert.setContentText("""
//                    接触：
//
//                    """
//                    .concat(item.isRoot() ? "全部数据" : item.toStringWithType()));
//            alert.initOwner(App.app().getPrimaryStage());
//            alert.showAndWait().filter(t -> t == ButtonType.OK).ifPresent(t -> AppContext.runBlocking(() -> {
//                final TreeItem<Item> node = null == treeView.getSelectionModel().getSelectedItem()
//                        ? treeView.getRoot() : treeView.getSelectionModel().getSelectedItem();
//                TreeHelper.walkLeafs(node, (treeItem, itemValue) -> {
//                    if (null == itemValue || null == itemValue.provider) return;
//                    if (itemValue.provider.isDirectory()) return;
//                    if (itemValue.provider.getItemActionForTouch() == null) return;
//                    try {
//                        itemValue.provider.getItemActionForTouch().accept(itemValue, null);
//                    } catch (Throwable throwable) {
//                        throwable.printStackTrace();
//                    }
//                });
//                AppContext.toast("已接触");
//            }));
//        };
//    }
}
