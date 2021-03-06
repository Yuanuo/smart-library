package org.appxi.smartlib.app.explorer;

import javafx.beans.property.ObjectProperty;
import javafx.beans.property.SimpleObjectProperty;
import javafx.scene.control.Button;
import javafx.scene.control.Tooltip;
import javafx.scene.control.TreeItem;
import javafx.scene.input.KeyCode;
import javafx.scene.input.KeyCodeCombination;
import javafx.scene.input.KeyCombination;
import javafx.scene.layout.BorderPane;
import org.appxi.javafx.app.web.WebViewer;
import org.appxi.javafx.helper.FxHelper;
import org.appxi.javafx.settings.DefaultOptions;
import org.appxi.javafx.settings.SettingsList;
import org.appxi.javafx.visual.MaterialIcon;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.javafx.workbench.WorkbenchPart;
import org.appxi.javafx.workbench.WorkbenchPartController;
import org.appxi.prefs.UserPrefs;
import org.appxi.smartlib.FolderProvider;
import org.appxi.smartlib.Item;
import org.appxi.smartlib.ItemEvent;
import org.appxi.smartlib.app.item.ItemEx;
import org.appxi.smartlib.app.item.ItemRenderer;
import org.appxi.smartlib.article.ArticleProvider;
import org.appxi.smartlib.dao.ItemsDao;
import org.appxi.util.ext.RawVal;

import java.nio.file.Path;
import java.util.Arrays;
import java.util.List;
import java.util.Set;

public class LibraryExplorer extends WorkbenchPartController.SideView {
    LibraryTreeView treeView;

    public LibraryExplorer(WorkbenchPane workbench) {
        super(workbench);

        this.id.set("EXPLORER");
        this.title.set("资源管理器");
        this.tooltip.set("资源管理器");
        this.graphic.set(MaterialIcon.LOCAL_LIBRARY.graphic());
    }

    @Override
    public void postConstruct() {
        ItemActions.setupInitialize();
        //
        app.eventBus.addEventHandler(ItemEvent.VIEWING, event -> {
            final Item item = event.item;
            if (null == item || null == item.provider) return;
            if (!ItemActions.hasViewer(item.provider)) {
                locate(item);
                return;
            }

            if (!UserPrefs.recents.containsProperty(item.getPath())) {
                UserPrefs.recents.setProperty(item.getPath(), System.currentTimeMillis());
                this.treeView.refresh();
            }

            final ItemRenderer newViewer = ItemActions.getViewer(item.provider).apply(item);
            final WorkbenchPart.MainView oldViewer = workbench.findMainViewPart(newViewer.id().get());
            if (null != oldViewer) {
                workbench.selectMainView(oldViewer.id().get());
                if (oldViewer instanceof ItemRenderer itemRenderer)
                    FxHelper.runLater(() -> itemRenderer.navigate(item));
                return;
            }
            FxHelper.runLater(() -> {
                workbench.addWorkbenchPartAsMainView(newViewer, false);
                if (newViewer instanceof WebViewer webViewer) {
                    webViewer.setPosition(item);
                }
                workbench.selectMainView(newViewer.id().get());
            });
        });
        app.eventBus.addEventHandler(ItemEvent.EDITING, event -> {
            final Item item = event.item;
            if (null == item || null == item.provider) return;
            if (!ItemActions.hasEditor(item.provider)) {
                locate(item);
                return;
            }
            final ItemRenderer newEditor = ItemActions.getEditor(item.provider).apply(item);
            final WorkbenchPart.MainView oldEditor = workbench.findMainViewPart(newEditor.id().get());
            if (null != oldEditor) {
                workbench.selectMainView(oldEditor.id().get());
                return;
            }
            FxHelper.runLater(() -> {
                workbench.addWorkbenchPartAsMainView(newEditor, false);
                workbench.selectMainView(newEditor.id().get());
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
                    item = new ItemEx(FolderProvider.ONE);
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
            treeView.getSelectionModel().clearSelection();
            // remove from main-views
            FxHelper.runLater(() -> workbench.mainViews.removeTabs(tab ->
                    tab.getUserData() instanceof ItemRenderer c && c.item().getPath().startsWith(event.item.getPath())));
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
        //
        SettingsList.add(() -> {
            final List<RawVal<String>> list = Arrays.asList(
                    new RawVal<>("simple", "默认项目位置"),
                    new RawVal<>("advanced", "启动时选择项目位置")
            );
            final String usedVal = UserPrefs.prefsEx.getString("profile.mode", "simple");
            //
            final ObjectProperty<RawVal<String>> valueProperty = new SimpleObjectProperty<>();
            valueProperty.setValue(list.stream().filter(v -> usedVal.equalsIgnoreCase(v.value())).findFirst().orElse(list.get(0)));
            valueProperty.addListener((o, ov, nv) -> UserPrefs.prefsEx.setProperty("profile.mode", nv.value()).save());
            return new DefaultOptions<RawVal<String>>("项目位置", "切换项目位置选择方式", "PROJECT", true)
                    .setValues(list)
                    .setValueProperty(valueProperty);
        });
        SettingsList.add(() -> {
            final List<RawVal<String>> list = Arrays.asList(
                    new RawVal<>("view", "查看"),
                    new RawVal<>("edit", "编辑"),
                    new RawVal<>("none", "无操作")
            );
            final String usedVal = UserPrefs.prefs.getString("explorer.enterAction", "view");
            //
            final ObjectProperty<RawVal<String>> valueProperty = new SimpleObjectProperty<>();
            valueProperty.setValue(list.stream().filter(v -> usedVal.equalsIgnoreCase(v.value())).findFirst().orElse(list.get(0)));
            valueProperty.addListener((o, ov, nv) -> UserPrefs.prefs.setProperty("explorer.enterAction", nv.value()));
            return new DefaultOptions<RawVal<String>>("打开方式", "资源管理器默认双击动作", "资源管理器", true)
                    .setValues(list)
                    .setValueProperty(valueProperty);
        });
    }

    @Override
    public void createViewport(BorderPane viewport) {
        super.createViewport(viewport);
        //
        app.getPrimaryScene().getAccelerators().put(
                new KeyCodeCombination(KeyCode.E, KeyCombination.SHORTCUT_DOWN),
                () -> workbench.selectSideTool(this.id.getValue()));
        //
        final Button btnNewArticle = new Button();
        btnNewArticle.setTooltip(new Tooltip("新建图文（Ctrl+N）"));
        btnNewArticle.setGraphic(MaterialIcon.POST_ADD.graphic());
        btnNewArticle.getStyleClass().add("flat");
        if (ItemActions.hasCreator(ArticleProvider.ONE)) {
            btnNewArticle.setOnAction(event -> ItemActions.getCreator(ArticleProvider.ONE).accept(preferFolder()));
        } else {
            btnNewArticle.setDisable(true);
        }
        app.getPrimaryScene().getAccelerators().put(
                new KeyCodeCombination(KeyCode.N, KeyCombination.SHORTCUT_DOWN),
                btnNewArticle::fire);
        //
        final Button btnNewFolder = new Button();
        btnNewFolder.setTooltip(new Tooltip("新建目录（Ctrl+Shift+N）"));
        btnNewFolder.setGraphic(MaterialIcon.CREATE_NEW_FOLDER.graphic());
        btnNewFolder.getStyleClass().add("flat");
        if (ItemActions.hasCreator(FolderProvider.ONE)) {
            btnNewFolder.setOnAction(event -> ItemActions.getCreator(FolderProvider.ONE).accept(preferFolder()));
        } else {
            btnNewFolder.setDisable(true);
        }
        app.getPrimaryScene().getAccelerators().put(
                new KeyCodeCombination(KeyCode.N, KeyCombination.SHORTCUT_DOWN, KeyCombination.SHIFT_DOWN),
                btnNewFolder::fire);
        //
        final Button btnLocate = new Button();
        btnLocate.setTooltip(new Tooltip("定位到..."));
        btnLocate.setGraphic(MaterialIcon.GPS_FIXED.graphic());
        btnLocate.getStyleClass().add("flat");
        btnLocate.setOnAction(event -> {
            final WorkbenchPart.MainView controller1 = workbench.getSelectedMainViewPart();
            if (controller1 instanceof ItemRenderer controller) {
                locate(controller.item());
            }
        });
        //
        this.topBar.addRight(btnNewArticle, btnNewFolder, btnLocate);
        //
        this.treeView = new LibraryTreeView(this);
        viewport.setCenter(this.treeView);
    }

    @Override
    public void activeViewport(boolean firstTime) {
        if (firstTime) {
            reload(null);
        }
    }

    public Item preferItem() {
        TreeItem<Item> treeItem = treeView.getSelectionModel().getSelectedItem();
        Item result = null == treeItem ? null : treeItem.getValue();

        if (null == result && workbench.getSelectedMainViewPart() instanceof ItemRenderer c) result = c.item();
        if (null == result) result = treeView.getRoot().getValue();
        return result;
    }

    public Item preferFolder() {
        Item result = preferItem();
        if (result.provider.isFolder()) return result;
        if (!ItemsDao.items().exists(result.getPath())) return treeView.root().getValue();

        Path path = Path.of(result.getPath());
        path = path.getParent();
        if (null == path) return treeView.getRoot().getValue();
        return new ItemEx(path.getFileName().toString(), path.toString().replace('\\', '/'), FolderProvider.ONE);
    }

    void locate(Item item) {
        final LibraryTreeItem treeItem = treeView.findTreeItem(item.getPath());
        if (null != treeItem) {
            treeView.getSelectionModel().select(treeItem);
            treeView.scrollToIfNotVisible(treeItem);
        }
    }

    void reload(LibraryTreeItem treeItem) {
        if (null == treeItem) treeItem = treeView.root();
        if (null == treeItem) {
            final LibraryTreeItem rootItem = new LibraryTreeItem(ItemEx.ROOT);
            rootItem.setExpanded(true);
            FxHelper.runLater(() -> this.treeView.setRoot(rootItem));
        } else {
            treeItem.reset();
            treeItem.setExpanded(true);
        }
    }
}
