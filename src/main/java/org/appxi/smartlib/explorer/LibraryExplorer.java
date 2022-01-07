package org.appxi.smartlib.explorer;

import javafx.event.Event;
import javafx.scene.control.Button;
import javafx.scene.control.Tab;
import javafx.scene.control.Tooltip;
import javafx.scene.control.TreeItem;
import javafx.scene.input.KeyCode;
import javafx.scene.input.KeyCodeCombination;
import javafx.scene.input.KeyCombination;
import org.appxi.javafx.helper.FxHelper;
import org.appxi.javafx.visual.MaterialIcon;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.javafx.workbench.WorkbenchViewController;
import org.appxi.javafx.workbench.views.WorkbenchSideViewController;
import org.appxi.prefs.UserPrefs;
import org.appxi.smartlib.item.FolderProvider;
import org.appxi.smartlib.item.Item;
import org.appxi.smartlib.item.ItemController;
import org.appxi.smartlib.item.ItemEditor;
import org.appxi.smartlib.item.ItemEvent;
import org.appxi.smartlib.item.ItemViewer;
import org.appxi.smartlib.item.article.ArticleProvider;

import java.nio.file.Path;
import java.util.Set;

public class LibraryExplorer extends WorkbenchSideViewController {
    LibraryTreeView treeView;

    public LibraryExplorer(WorkbenchPane workbench) {
        super("EXPLORER", workbench);
        this.setTitles("资源管理器");
        this.viewGraphic.set(MaterialIcon.LOCAL_LIBRARY.graphic());
    }

    @Override
    public void initialize() {
        app.eventBus.addEventHandler(ItemEvent.VIEWING, event -> {
            final Item item = event.item;
            if (null == item || null == item.provider) return;
            if (null == item.provider.getViewer()) {
                locate(item);
                return;
            }

            if (!UserPrefs.recents.containsProperty(item.getPath())) {
                UserPrefs.recents.setProperty(item.getPath(), System.currentTimeMillis());
                this.treeView.refresh();
            }

            final ItemViewer newViewer = item.provider.getViewer().apply(item);
            final ItemViewer oldViewer = (ItemViewer) workbench.findMainViewController(newViewer.viewId.get());
            if (null != oldViewer) {
                workbench.selectMainView(oldViewer.viewId.get());
                FxHelper.runLater(() -> oldViewer.navigate(item));
                return;
            }
            FxHelper.runLater(() -> {
                workbench.addWorkbenchViewAsMainView(newViewer, false);
                newViewer.initialize();
                newViewer.attr(Item.class, item);
                workbench.selectMainView(newViewer.viewId.get());
            });
        });
        app.eventBus.addEventHandler(ItemEvent.EDITING, event -> {
            final Item item = event.item;
            if (null == item || null == item.provider) return;
            if (null == item.provider.getEditor()) {
                locate(item);
                return;
            }
            final ItemEditor newEditor = item.provider.getEditor().apply(item);
            final ItemEditor oldEditor = (ItemEditor) workbench.findMainViewController(newEditor.viewId.get());
            if (null != oldEditor) {
                workbench.selectMainView(oldEditor.viewId.get());
                return;
            }
            FxHelper.runLater(() -> {
                workbench.addWorkbenchViewAsMainView(newEditor, false);
                newEditor.initialize();
                workbench.selectMainView(newEditor.viewId.get());
            });
        });
        //
        app.eventBus.addEventHandler(ItemEvent.CREATED, event -> {
            final String[] paths = event.item.getPath().split("/");
            TreeItem<Item> node = treeView.root();
            String path = null;
            Item item;
            for (int i = 0; i < paths.length; i++) {
                if (i == paths.length - 1) {
                    item = event.item;
                } else {
                    item = new Item(FolderProvider.ONE);
                    item.setName(paths[i]);
                    if (null == path) item.setPath(paths[i]);
                    else item.setPath(path.concat("/").concat(paths[i]));
                }
                //
                path = item.getPath();
                TreeItem<Item> child = ((LibraryTreeItem) node).findTreeItem(path);
                if (null == child)
                    node.getChildren().add(child = new LibraryTreeItem(item));
                node = child;
            }
            node.setExpanded(true);
            treeView.getSelectionModel().select(node);
            treeView.scrollToIfNotVisible(node);
            app.eventBus.fireEvent(new ItemEvent(ItemEvent.EDITING, event.item));
        });
        //
        app.eventBus.addEventHandler(ItemEvent.RENAMED, event -> {
            // update in tree-view
            LibraryTreeItem treeItem = (LibraryTreeItem) treeView.getSelectionModel().getSelectedItem();
            treeItem.rebase(event.from.getPath(), event.item.getPath());
            treeItem.resort();
            treeView.getSelectionModel().select(treeItem);
            treeView.scrollToIfNotVisible(treeItem);
            // update in recents
            Set.copyOf(UserPrefs.recents.getPropertyKeys()).forEach(k -> {
                if (k.startsWith(event.from.getPath())) {
                    final Object v = UserPrefs.recents.removeProperty(k);
                    UserPrefs.recents.setProperty(k.replace(event.from.getPath(), event.item.getPath()), v);
                }
            });
        });
        app.eventBus.addEventHandler(ItemEvent.MOVED, event -> {
            LibraryTreeItem sourceItem = (LibraryTreeItem) treeView.getSelectionModel().getSelectedItem();
            LibraryTreeItem targetParent = treeView.findTreeItem(event.item.parentPath());
            // remove from previous location
            sourceItem.getParent().getChildren().remove(sourceItem);
            //
            sourceItem.rebase(event.from.getPath(), event.item.getPath());

            // add to new location
            LibraryTreeItem selected = targetParent.findTreeItem(event.item.getPath());
            if (null == selected)
                targetParent.getChildren().add(selected = sourceItem);
            treeView.getSelectionModel().select(selected);
            treeView.scrollToIfNotVisible(selected);
            //
            Set.copyOf(UserPrefs.recents.getPropertyKeys()).forEach(k -> {
                if (k.startsWith(event.from.getPath())) {
                    final Object v = UserPrefs.recents.removeProperty(k);
                    UserPrefs.recents.setProperty(k.replaceFirst(event.from.getPath(), event.item.getPath()), v);
                }
            });
        });
        app.eventBus.addEventHandler(ItemEvent.DELETED, event -> {
            // remove from tree-view
            final TreeItem<Item> node = null == treeView.getSelectionModel().getSelectedItem()
                    ? treeView.getRoot() : treeView.getSelectionModel().getSelectedItem();
            if (null == node.getParent())
                node.getChildren().clear();
            else node.getParent().getChildren().remove(node);
            // remove from main-views
            workbench.getMainViewsTabs().forEach(tab -> {
                if (!(tab.getUserData() instanceof ItemController itemView))
                    return;
                if (!itemView.item.getPath().startsWith(event.item.getPath()))
                    return;
                FxHelper.runLater(() -> tab.getTabPane().getTabs().remove(tab));
                if (tab.getOnClosed() != null) {
                    Event.fireEvent(tab, new Event(Tab.CLOSED_EVENT));
                }
            });
            // remove from recents
            Set.copyOf(UserPrefs.recents.getPropertyKeys()).forEach(k -> {
                if (k.startsWith(event.item.getPath())) {
                    UserPrefs.recents.removeProperty(k);
                }
            });
        });
        app.eventBus.addEventHandler(ItemEvent.RESTORED, event -> {
            LibraryTreeItem treeItem = (LibraryTreeItem) treeView.getSelectionModel().getSelectedItem();
            reload(treeItem);
        });
    }

    @Override
    protected void onViewportInitOnce() {
        //
        app.getPrimaryScene().getAccelerators().put(
                new KeyCodeCombination(KeyCode.E, KeyCombination.SHORTCUT_DOWN),
                () -> workbench.selectSideTool(this.viewId.getValue()));
        //
        final Button btnNewArticle = new Button();
        btnNewArticle.setTooltip(new Tooltip("新建图文（Ctrl+N）"));
        btnNewArticle.setGraphic(MaterialIcon.POST_ADD.graphic());
        btnNewArticle.getStyleClass().add("flat");
        btnNewArticle.setOnAction(event -> ArticleProvider.ONE.getCreator().accept(preferFolder()));
        app.getPrimaryScene().getAccelerators().put(
                new KeyCodeCombination(KeyCode.N, KeyCombination.SHORTCUT_DOWN),
                btnNewArticle::fire);
        //
        final Button btnNewFolder = new Button();
        btnNewFolder.setTooltip(new Tooltip("新建目录（Ctrl+Shift+N）"));
        btnNewFolder.setGraphic(MaterialIcon.CREATE_NEW_FOLDER.graphic());
        btnNewFolder.getStyleClass().add("flat");
        btnNewFolder.setOnAction(event -> FolderProvider.ONE.getCreator().accept(preferFolder()));
        app.getPrimaryScene().getAccelerators().put(
                new KeyCodeCombination(KeyCode.N, KeyCombination.SHORTCUT_DOWN, KeyCombination.SHIFT_DOWN),
                btnNewFolder::fire);
        //
        final Button btnLocate = new Button();
        btnLocate.setTooltip(new Tooltip("定位到..."));
        btnLocate.setGraphic(MaterialIcon.GPS_FIXED.graphic());
        btnLocate.getStyleClass().add("flat");
        btnLocate.setOnAction(event -> {
            final WorkbenchViewController controller1 = workbench.getSelectedMainViewController();
            if (controller1 instanceof ItemController controller) locate(controller.item);
        });
        //
        this.topBar.addRight(btnNewArticle, btnNewFolder, btnLocate);
        //
        this.treeView = new LibraryTreeView(this);
        this.viewport.setCenter(this.treeView);
    }

    //    private final Item helpItem = ItemLinkProvider.ofFile("@HELP", "${appDir}/help/index-${locale}.html").setTitle("Help...");

    @Override
    public void onViewportShowing(boolean firstTime) {
        if (firstTime) reload(null);
    }

    @Override
    public void onViewportHiding() {
    }

    public Item preferItem() {
        TreeItem<Item> treeItem = treeView.getSelectionModel().getSelectedItem();
        Item result = null == treeItem ? null : treeItem.getValue();

        if (null == result && workbench.getSelectedMainViewController() instanceof ItemController c) result = c.item;
        if (null == result) result = treeView.getRoot().getValue();
        return result;
    }

    public Item preferFolder() {
        Item result = preferItem();
        if (result.provider.isDirectory()) return result;

        Path path = Path.of(result.getPath());
        path = path.getParent();
        if (null == path) return treeView.getRoot().getValue();
        return new Item(path.getFileName().toString(), path.toString().replace('\\', '/'), FolderProvider.ONE);
    }

    void locate(Item item) {
        final LibraryTreeItem treeItem = treeView.findTreeItem(item.getPath());
        if (null != treeItem) {
            treeView.getSelectionModel().select(treeItem);
            treeView.scrollToIfNotVisible(treeItem);
        }
    }

    void reload(LibraryTreeItem treeItem) {
        if (null == treeItem) {
            final LibraryTreeItem rootItem = new LibraryTreeItem(Item.ROOT);
            this.treeView.setRoot(rootItem);
            rootItem.setExpanded(true);
        } else {
            treeItem.reset();
            treeItem.setExpanded(true);
        }
    }
}
