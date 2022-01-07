package org.appxi.smartlib.explorer;

import javafx.scene.control.ContextMenu;
import javafx.scene.control.MenuItem;
import javafx.scene.control.SeparatorMenuItem;
import javafx.scene.control.TreeItem;
import javafx.scene.input.ContextMenuEvent;
import javafx.scene.input.KeyCode;
import javafx.scene.input.KeyEvent;
import javafx.scene.input.MouseButton;
import org.appxi.javafx.app.BaseApp;
import org.appxi.javafx.control.TreeViewEx;
import org.appxi.javafx.visual.MaterialIcon;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.prefs.UserPrefs;
import org.appxi.smartlib.dao.DataApi;
import org.appxi.smartlib.event.SearcherEvent;
import org.appxi.smartlib.item.Item;
import org.appxi.smartlib.item.ItemEvent;
import org.appxi.smartlib.item.ItemProviders;

import java.awt.*;
import java.util.ArrayList;
import java.util.List;

class LibraryTreeView extends TreeViewEx<Item> {
    final BaseApp app;
    final WorkbenchPane workbench;
    final LibraryExplorer explorer;

    private ContextMenu contextMenu;

    public LibraryTreeView(LibraryExplorer explorer) {
        super();
        this.app = explorer.app;
        this.workbench = explorer.workbench;
        this.explorer = explorer;

        this.getStyleClass().add("explorer");
        this.setEnterOrDoubleClickAction((e, t) -> {
            String action = UserPrefs.prefs.getString("explorer.enterAction", "view");
            if ("none".equals(action)) return;
            final Item item = t.getValue();
            if ("edit".equals(action) && null != item.provider.getEditor())
                app.eventBus.fireEvent(new ItemEvent(ItemEvent.EDITING, item));
            else if (null != item.provider.getViewer())
                app.eventBus.fireEvent(new ItemEvent(ItemEvent.VIEWING, item));
        });

        //
        this.setCellFactory(new LibraryTreeCell());
        // dynamic context-menu
        this.setOnContextMenuRequested(this::handleOnContextMenuRequested);
        // hide context-menu
        this.setOnMousePressed(event -> {
            if (event.getButton() != MouseButton.SECONDARY && null != contextMenu && contextMenu.isShowing())
                contextMenu.hide();
        });
        //
        this.addEventHandler(KeyEvent.KEY_RELEASED, event -> {
            if (event.getCode() == KeyCode.F2) { // rename
                final TreeItem<Item> treeItem = this.getSelectionModel().getSelectedItem();
                if (null != treeItem) ItemActions.rename(treeItem.getValue());
            }
        });
    }

    private void handleOnContextMenuRequested(ContextMenuEvent event) {
        final LibraryTreeItem treeItem = (LibraryTreeItem) this.getSelectionModel().getSelectedItem();
        final Item item = null == treeItem ? null : treeItem.getValue();
        final List<MenuItem> menuItems = new ArrayList<>(16);
        //
        if (null != item) {
            if (item.provider.getViewer() != null) {
                // view
                final MenuItem menuItem = new MenuItem("查看");
                menuItem.setGraphic(MaterialIcon.VISIBILITY.graphic());
                menuItem.setOnAction(e -> app.eventBus.fireEvent(new ItemEvent(ItemEvent.VIEWING, item)));
                menuItems.add(menuItem);
            }
            if (item.provider.getEditor() != null) {
                // edit
                final MenuItem menuItem = new MenuItem("编辑");
                menuItem.setGraphic(MaterialIcon.EDIT.graphic());
                menuItem.setOnAction(e -> app.eventBus.fireEvent(new ItemEvent(ItemEvent.EDITING, item)));
                menuItems.add(menuItem);
            }

            // rename
            final MenuItem rename = new MenuItem("重命名");
            rename.setGraphic(MaterialIcon.TEXT_FIELDS.graphic());
            rename.setOnAction(e -> ItemActions.rename(item));
            menuItems.add(rename);
            // delete
            final MenuItem delete = new MenuItem("删除");
            delete.setGraphic(MaterialIcon.CLEAR.graphic());
            delete.setOnAction(e -> ItemActions.delete(item));
            menuItems.add(delete);

            // search in this
            if (item.provider.isDirectory()) {
                final MenuItem menuItem = new MenuItem("从这里搜索");
                menuItem.setGraphic(MaterialIcon.FIND_IN_PAGE.graphic());
                menuItem.setOnAction(e -> app.eventBus.fireEvent(SearcherEvent.ofSearch(null, item)));
                menuItems.add(menuItem);
            }
        }
        if (!menuItems.isEmpty()) {
            // ---
            menuItems.add(new SeparatorMenuItem());
        }
        // new supported items
        ItemProviders.providers().forEach(provider -> {
            if (null == provider.getCreator()) return;
            final MenuItem menuItem = new MenuItem("新建 ".concat(provider.providerName()));
            menuItem.setGraphic(MaterialIcon.ADD.graphic());
            menuItem.setOnAction(e -> provider.getCreator().accept(explorer.preferFolder()));
            menuItems.add(menuItem);
        });
        if (!menuItems.isEmpty()) {
            // ---
            menuItems.add(new SeparatorMenuItem());
        }
        // backup
        final MenuItem backup = new MenuItem("备份／导出数据");
        backup.setGraphic(MaterialIcon.CALL_SPLIT.graphic());
        backup.setOnAction(e -> ItemActions.backup(null != item ? item : root().getValue()));
        menuItems.add(backup);
        // restore
        final MenuItem restore = new MenuItem("还原／导入数据");
        restore.setGraphic(MaterialIcon.CALL_MERGE.graphic());
        restore.setOnAction(e -> ItemActions.restore(explorer.preferFolder()));
        menuItems.add(restore);
        // ---
        menuItems.add(new SeparatorMenuItem());
        // reindex
        if (null != item) {
            final MenuItem reindex = new MenuItem("重建索引");
            reindex.setGraphic(MaterialIcon.FIND_REPLACE.graphic());
            reindex.setOnAction(e -> ItemActions.reindex(item));
            menuItems.add(reindex);
            // ---
//            if (item.provider.getTouch() != null) {
//                final MenuItem menuItem = new MenuItem("接触");
//                menuItem.setGraphic(MaterialIcon.TOUCH_APP.graphic());
//                menuItem.setOnAction(e -> item.provider.getTouch().accept(item, this));
//                menuItems.add(menuItem);
//            }
            // ---
        } else {
            final MenuItem reindex = new MenuItem("重建全部索引");
            reindex.setGraphic(MaterialIcon.FIND_REPLACE.graphic());
            reindex.setOnAction(e -> ItemActions.reindex(root().getValue()));
            menuItems.add(reindex);
            //
            final MenuItem deleteAll = new MenuItem("删除全部数据");
            deleteAll.setGraphic(MaterialIcon.CLEAR.graphic());
            deleteAll.setOnAction(e -> ItemActions.delete(root().getValue()));
            menuItems.add(deleteAll);
        }
        //
        final MenuItem reload = new MenuItem("刷新");
        reload.setGraphic(MaterialIcon.SYNC.graphic());
        reload.setOnAction(e -> explorer.reload(null != treeItem ? treeItem : root()));
        menuItems.add(reload);
        //
        if (Desktop.isDesktopSupported() && Desktop.getDesktop().isSupported(Desktop.Action.OPEN)) {
            final MenuItem place = new MenuItem("显示本地文件位置");
            place.setGraphic(MaterialIcon.PLACE.graphic());

            place.setOnAction(e -> {
                if (null == item || item.provider.isDirectory()) {
                    try {
                        Desktop.getDesktop().open(DataApi.dataAccess().filePath(item));
                    } catch (Throwable ex) {
                        ex.printStackTrace();
                    }
                } else if (Desktop.getDesktop().isSupported(Desktop.Action.BROWSE_FILE_DIR)) {
                    Desktop.getDesktop().browseFileDirectory(DataApi.dataAccess().filePath(item));
                } else {
                    try {
                        Desktop.getDesktop().open(DataApi.dataAccess().filePath(item).getParentFile());
                    } catch (Throwable ex) {
                        ex.printStackTrace();
                    }
                }
            });
            menuItems.add(place);
        }
        // ---
        menuItems.add(new SeparatorMenuItem());
        // unselect
        final MenuItem unselect = new MenuItem("取消选择（ESC）");
        unselect.setOnAction(e -> this.getSelectionModel().clearSelection());
        menuItems.add(unselect);

        //
        event.consume();
        if (null != contextMenu && contextMenu.isShowing())
            contextMenu.hide();
        contextMenu = new ContextMenu(menuItems.toArray(new MenuItem[0]));
        contextMenu.show(this, event.getScreenX(), event.getScreenY());
    }

    public LibraryTreeItem root() {
        return (LibraryTreeItem) this.getRoot();
    }

    public LibraryTreeItem findTreeItem(String path) {
        return "".equals(path) ? this.root() : this.root().findTreeItem(path);
    }
}
