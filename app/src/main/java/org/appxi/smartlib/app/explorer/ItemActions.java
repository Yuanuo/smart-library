package org.appxi.smartlib.app.explorer;

import javafx.application.Platform;
import javafx.scene.control.Alert;
import javafx.scene.control.ButtonType;
import javafx.scene.control.DialogPane;
import javafx.scene.control.TextArea;
import javafx.scene.control.TextInputDialog;
import javafx.scene.control.TreeItem;
import javafx.stage.FileChooser;
import org.apache.commons.io.FilenameUtils;
import org.appxi.holder.BoolHolder;
import org.appxi.javafx.control.ProgressLayer;
import org.appxi.javafx.helper.FxHelper;
import org.appxi.prefs.UserPrefs;
import org.appxi.smartlib.FolderProvider;
import org.appxi.smartlib.Item;
import org.appxi.smartlib.ItemEvent;
import org.appxi.smartlib.ItemHelper;
import org.appxi.smartlib.ItemProvider;
import org.appxi.smartlib.ItemProviders;
import org.appxi.smartlib.app.App;
import org.appxi.smartlib.app.item.ItemRenderer;
import org.appxi.smartlib.app.item.article.ArticleEditor;
import org.appxi.smartlib.app.item.article.ArticleViewer;
import org.appxi.smartlib.app.item.mindmap.MindmapEditor;
import org.appxi.smartlib.app.item.mindmap.MindmapViewer;
import org.appxi.smartlib.app.item.tika.TikaViewer;
import org.appxi.smartlib.article.ArticleProvider;
import org.appxi.smartlib.dao.BeansContext;
import org.appxi.smartlib.dao.ItemsDao;
import org.appxi.smartlib.dao.PiecesRepository;
import org.appxi.smartlib.mindmap.MindmapProvider;
import org.appxi.smartlib.tika.DocProvider;
import org.appxi.smartlib.tika.DocxProvider;
import org.appxi.smartlib.tika.PdfProvider;
import org.appxi.smartlib.tika.PptProvider;
import org.appxi.smartlib.tika.PptxProvider;
import org.appxi.smartlib.tika.TikaProvider;
import org.appxi.smartlib.tika.TxtProvider;
import org.appxi.smartlib.tika.XlsProvider;
import org.appxi.smartlib.tika.XlsxProvider;
import org.appxi.util.DateHelper;
import org.appxi.util.FileHelper;
import org.appxi.util.ext.Attributes;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.awt.*;
import java.io.File;
import java.io.FileOutputStream;
import java.util.Arrays;
import java.util.Comparator;
import java.util.Date;
import java.util.HashSet;
import java.util.List;
import java.util.Set;
import java.util.function.Consumer;
import java.util.function.Function;
import java.util.stream.Collectors;
import java.util.zip.ZipFile;
import java.util.zip.ZipOutputStream;

public class ItemActions {
    private static final Logger logger = LoggerFactory.getLogger(ItemActions.class);

    private static final Object AK_CREATOR = new Object();
    private static final Object AK_EDITOR = new Object();
    private static final Object AK_VIEWER = new Object();

    public static final Comparator<? super TreeItem<Item>> sortByName = Comparator.comparing(v -> ItemHelper.toComparableString(v.getValue()));

    public static void setupInitialize() {
        ItemProviders.add(ArticleProvider.ONE,
                MindmapProvider.ONE,
                DocxProvider.ONE, DocProvider.ONE,
                PptxProvider.ONE, PptProvider.ONE,
                XlsxProvider.ONE, XlsProvider.ONE,
                PdfProvider.ONE,
                TxtProvider.ONE
        );
        //
        //
        FolderProvider.ONE.attr(AK_CREATOR, (Consumer<Item>) parent -> ItemActions.create(FolderProvider.ONE, parent));

        // article
        ArticleProvider.ONE.attr(AK_CREATOR, (Consumer<Item>) parent -> ItemActions.create(ArticleProvider.ONE, parent));
        ArticleProvider.ONE.attr(AK_EDITOR, (Function<Item, ItemRenderer>) item -> new ArticleEditor(item, App.app().workbench()));
        ArticleProvider.ONE.attr(AK_VIEWER, (Function<Item, ItemRenderer>) item -> new ArticleViewer(item, App.app().workbench()));

        // mindmap
        MindmapProvider.ONE.attr(AK_EDITOR, (Function<Item, ItemRenderer>) item -> new MindmapEditor(item, App.app().workbench()));
        MindmapProvider.ONE.attr(AK_VIEWER, (Function<Item, ItemRenderer>) item -> new MindmapViewer(item, App.app().workbench()));

        // tika
        ItemProviders.list().forEach(provider -> {
            if (provider instanceof TikaProvider tikaProvider) {
                tikaProvider.attr(AK_VIEWER, (Function<Item, ItemRenderer>) item -> new TikaViewer(item, App.app().workbench()));
            }
        });
    }

    public static boolean hasCreator(ItemProvider itemProvider) {
        if (itemProvider instanceof Attributes attr) {
            return attr.hasAttr(AK_CREATOR);
        }
        return false;
    }

    public static boolean hasEditor(ItemProvider itemProvider) {
        if (itemProvider instanceof Attributes attr) {
            return attr.hasAttr(AK_EDITOR);
        }
        return false;
    }

    public static boolean hasViewer(ItemProvider itemProvider) {
        if (itemProvider instanceof Attributes attr) {
            return attr.hasAttr(AK_VIEWER);
        }
        return false;
    }

    public static Consumer<Item> getCreator(ItemProvider itemProvider) {
        if (itemProvider instanceof Attributes attr) {
            return attr.attr(AK_CREATOR);
        }
        return null;
    }

    public static Function<Item, ItemRenderer> getEditor(ItemProvider itemProvider) {
        if (itemProvider instanceof Attributes attr) {
            return attr.attr(AK_EDITOR);
        }
        return null;
    }

    public static Function<Item, ItemRenderer> getViewer(ItemProvider itemProvider) {
        if (itemProvider instanceof Attributes attr) {
            return attr.attr(AK_VIEWER);
        }
        return null;
    }

    public static void create(ItemProvider itemProvider, Item parent) {
        final TextInputDialog dialog = new TextInputDialog(parent.isRoot() ? "/" : "/".concat(parent.getPath()).concat("/"));
        dialog.setTitle("添加 ".concat(itemProvider.providerName()));
        dialog.setHeaderText("""
                请输入有效的名称!
                1) 名称使用标准字符（数字、字母、文字），请勿使用特殊字符！
                2) 使用斜杠（半角/）分隔可创建多级目录，如“/A/B/C”！
                                    
                """);
        final BoolHolder hacked = new BoolHolder(false);
        dialog.getEditor().focusedProperty().addListener((o, ov, nv) -> {
            if (nv && !hacked.value) {
                hacked.value = true;
                Platform.runLater(() -> dialog.getEditor().end());
            }
        });
        dialog.getDialogPane().setPrefWidth(800);
        dialog.initOwner(App.app().getPrimaryStage());
        dialog.showAndWait().ifPresent(str -> {
            str = str.strip().replace("\\", "/");
            str = str.replace("//", "/").replaceAll("/$", "");

            if (str.startsWith("/")) str = str.substring(1);
            else str = parent.isRoot() ? str : parent.getPath().concat("/").concat(str);

            if (str.isEmpty()) return;
            str = Arrays.stream(str.split("/"))
                    .map(s -> FileHelper.toValidName(s.replaceAll("^[.]", "")).strip())
                    .filter(s -> !s.isBlank())
                    .collect(Collectors.joining("/"))
                    .replace("//", "/")
                    .replaceFirst("/$", "");
            //
            Item item = new Item(itemProvider);
            item.setName(FilenameUtils.getName(str));
            item.setPath(str);
            //
            final String msg = ItemsDao.items().create(item);
            if (null != msg) {
                App.app().toastError(msg);
                return;
            }
            App.app().eventBus.fireEvent(new ItemEvent(ItemEvent.CREATED, item));
        });
    }

    static void rename(Item item) {
        final TextInputDialog dialog = new TextInputDialog(item.getName());
        dialog.setTitle("重命名");
        dialog.setHeaderText(item.toDetail());
        dialog.getDialogPane().setPrefWidth(800);
        dialog.initOwner(App.app().getPrimaryStage());
        dialog.showAndWait().ifPresent(str -> {
            str = FileHelper.toValidName(str.replaceAll("^[.]", "")).strip();
            if (str.isEmpty() || str.equals(item.getName()))
                return;
            final String newName = str;
            ProgressLayer.showAndWait(App.app().getPrimaryGlass(), progressLayer -> {
                final Item oldItem = item.clone(), newItem = item.clone();
                final String msg = ItemsDao.items().rename(newItem, newName);
                if (msg != null) {
                    App.app().toastError(msg);
                    return;
                }
                // update indexes
                final PiecesRepository repository = BeansContext.getBean(PiecesRepository.class);
                if (null != repository) {
                    repository.deleteAllByPath(item.getPath());
                    ItemsDao.items().reindex(newItem, (d, s) -> Platform.runLater(() -> progressLayer.message.setText(s)));
                }
                FxHelper.runLater(() -> {
                    item.setName(newItem.getName()).setPath(newItem.getPath());
                    //
                    App.app().eventBus.fireEvent(new ItemEvent(ItemEvent.RENAMED, item, oldItem));
                });
            });
        });
    }

    static void delete(Item item) {
        final Alert dialog = new Alert(Alert.AlertType.CONFIRMATION);
        dialog.setTitle("删除");
        if (Desktop.isDesktopSupported() && Desktop.getDesktop().isSupported(Desktop.Action.MOVE_TO_TRASH)) {
            dialog.setHeaderText("此操作不可恢复！（将删除到回收站）");
        } else {
            dialog.setHeaderText("此操作不可恢复！");
        }
        dialog.setContentText("将删除：\n".concat(item.toDetail()));
        dialog.initOwner(App.app().getPrimaryStage());
        dialog.getDialogPane().setPrefWidth(800);
        dialog.showAndWait().filter(t -> t == ButtonType.OK).ifPresent(t -> ProgressLayer.showAndWait(App.app().getPrimaryGlass(), progressLayer -> {
            final String msg = ItemsDao.items().delete(item, (d, s) -> Platform.runLater(() -> progressLayer.message.setText(s)));
            if (msg != null) {
                App.app().toastError(msg);
                return;
            }
            // update indexes
            final PiecesRepository repository = BeansContext.getBean(PiecesRepository.class);
            if (null != repository) {
                if (item.provider.isFolder()) {
                    if (item.isRoot()) repository.deleteAll();
                    else repository.deleteAllByPath(item.getPath());
                } else {
                    repository.deleteAllByPath(item.getPath());
                }
            }
            //
            App.app().eventBus.fireEvent(new ItemEvent(ItemEvent.DELETED, item));
        }));
    }

    static void move(Item item, Item newParent) {
        final Item oldItem = item.clone(), newItem = item.clone();
        final Runnable moving = () -> ProgressLayer.showAndWait(App.app().getPrimaryGlass(), progressLayer -> {
            final String msg = ItemsDao.items().move(newItem, newParent);
            if (msg != null) {
                App.app().toastError(msg);
                return;
            }

            // update indexes
            final PiecesRepository repository = BeansContext.getBean(PiecesRepository.class);
            if (null != repository) {
                repository.deleteAllByPath(item.getPath());
                ItemsDao.items().reindex(newItem, (d, s) -> Platform.runLater(() -> progressLayer.message.setText(s)));
            }

            FxHelper.runLater(() -> {
                item.setName(newItem.getName()).setPath(newItem.getPath());
                //
                App.app().eventBus.fireEvent(new ItemEvent(ItemEvent.MOVED, item, oldItem));
            });
        });

        List<String> exists = ItemsDao.items().exists(item.getPath(), newParent.getPath());
        if (null == exists || exists.isEmpty()) {
            moving.run();
            return;
        }

        TextArea textArea = new TextArea(String.join("\n", exists));
        textArea.setWrapText(false);
        textArea.setEditable(false);

        DialogPane dialogPane = new DialogPane();
        dialogPane.setHeaderText("""
                将：${source}
                移动到：${target}/
                以下项目将被覆盖，请确认是否继续？
                """
                .replace("${source}", item.toDetail())
                .replace("${target}", newParent.toDetail()));
        dialogPane.setContent(textArea);
        dialogPane.getButtonTypes().addAll(ButtonType.CANCEL, ButtonType.OK);

        final Alert dialog = new Alert(Alert.AlertType.CONFIRMATION);
        dialog.setTitle("移动");
        dialog.setDialogPane(dialogPane);
        dialog.getDialogPane().setPrefWidth(800);
        dialog.initOwner(App.app().getPrimaryStage());
        dialog.showAndWait().filter(t -> t == ButtonType.OK).ifPresent(t -> moving.run());
    }

    static void reindex(Item item) {
        final Alert dialog = new Alert(Alert.AlertType.CONFIRMATION);
        dialog.setTitle("重建索引");
        dialog.setHeaderText("");
        dialog.setContentText("""
                此操作根据数据量可能耗时极长，若无必要请勿使用！

                若在索引过程中意外中止可能导致索引数据库损坏！！！
                （需关闭程序后手动删除数据目录中的“.solr”目录）

                将重建以下数据项及所有子项的索引数据：
                                
                """
                .concat(item.toDetail()));
        dialog.getDialogPane().setPrefWidth(800);
        dialog.initOwner(App.app().getPrimaryStage());
        dialog.showAndWait().filter(t -> t == ButtonType.OK)
                .ifPresent(t -> ProgressLayer.showAndWait(App.app().getPrimaryGlass(), progressLayer -> {
                    final String msg = ItemsDao.items().reindex(item, (d, s) -> Platform.runLater(() -> progressLayer.message.setText(s)));
                    if (msg != null) {
                        App.app().toastError(msg);
                        return;
                    }
                    App.app().toast("已重建索引");
                }));
    }

    static void backup(Item item) {
        final String initDir = UserPrefs.prefs.getString("exchange.dir", null);
        final String initFileName = (item.isRoot() ? "全部数据" : item.getName())
                .concat("-").concat(DateHelper.format3(new Date())).concat(".smartlib");

        final FileChooser fileChooser = new FileChooser();
        if (null != initDir)
            fileChooser.setInitialDirectory(new File(initDir));
        fileChooser.setInitialFileName(initFileName);
        fileChooser.setTitle("保存文件...");
        fileChooser.getExtensionFilters().add(new FileChooser.ExtensionFilter(App.NAME.concat("数据包"), "*.smartlib"));
        File file = fileChooser.showSaveDialog(App.app().getPrimaryStage());
        if (null == file)
            return;
        if (!file.getParentFile().canWrite()) {
            App.app().toastError("目录不能写入，请重新选择！");
            return;
        }
        UserPrefs.prefs.setProperty("exchange.dir", file.getParent());
        //
        if (file.exists())
            file = new File(file.getParentFile(), initFileName);
        //
        File finalFile = file;
        ProgressLayer.showAndWait(App.app().getPrimaryGlass(), progressLayer -> {
            try (ZipOutputStream zipOutput = new ZipOutputStream(new FileOutputStream(finalFile))) {
                final String msg = ItemsDao.items().backup(item, zipOutput, (d, s) -> Platform.runLater(() -> progressLayer.message.setText(s)));
                if (null != msg) {
                    App.app().toastError(msg);
                    return;
                }
                App.app().toast("已导出");
            } catch (Exception e) {
                logger.warn("backup", e);
                App.app().toastError(e.getMessage());
            }
        });
    }

    static void restore(Item item) {
        final String lastDir = UserPrefs.prefs.getString("exchange.dir", null);

        final FileChooser fileChooser = new FileChooser();
        if (null != lastDir)
            fileChooser.setInitialDirectory(new File(lastDir));
        fileChooser.setTitle("打开文件...");
        fileChooser.getExtensionFilters().add(new FileChooser.ExtensionFilter(App.NAME.concat("数据包"), "*.smartlib"));
        final List<File> files = fileChooser.showOpenMultipleDialog(App.app().getPrimaryStage());
        if (null == files || files.isEmpty())
            return;
        UserPrefs.prefs.setProperty("exchange.dir", files.get(0).getParent());
        //
        Alert dialog = new Alert(Alert.AlertType.CONFIRMATION, "此操作将覆盖已经存在的数据，确认继续？");
        dialog.getDialogPane().setPrefWidth(800);
        dialog.initOwner(App.app().getPrimaryStage());
        dialog.showAndWait().filter(t -> t == ButtonType.OK).ifPresent(t -> ProgressLayer.showAndWait(App.app().getPrimaryGlass(), progressLayer -> {
            final Set<String> rootNames = new HashSet<>();
            // unpack zip files
            for (File file : files) {
                try (ZipFile zipFile = new ZipFile(file)) {
                    final String msg = ItemsDao.items().restore(item, zipFile, (d, s) -> Platform.runLater(() -> progressLayer.message.setText(s)));
                    if (null != msg) {
                        App.app().toastError(msg);
                    }
                    rootNames.addAll(zipFile.stream().map(zipEntry -> zipEntry.getName().split("/", 2)[0])
                            .collect(Collectors.toSet()));
                } catch (Exception e) {
                    logger.error("restore", e);
                    App.app().toastError(e.getMessage());
                }
            }
            // 更新索引，避免重建父级全目录耗时太长
            String itemPath = item.getPath();
            itemPath = itemPath.isEmpty() ? "" : itemPath + "/";
            for (String rootName : rootNames) {
                Item rootItem = ItemsDao.items().resolve(itemPath + rootName);
                ItemsDao.items().reindex(rootItem, (d, s) -> Platform.runLater(() -> progressLayer.message.setText(s)));
            }

            App.app().toast("已导入");
            App.app().eventBus.fireEvent(new ItemEvent(ItemEvent.RESTORED, item));
        }));
    }
//
//    public static void setContent(Item item, InputStream content, boolean reindex) {
//        final String msg = ItemsDao.items().setContent(item, content);
//        if (msg != null) {
//            App.app().toastError(msg);
//            return;
//        }
//        // update indexes
//        if (reindex)
//            ItemsDao.items().reindex(item, (d, s) -> {
//            });
//    }

    public static void touch(Item item) {
        final Alert dialog = new Alert(Alert.AlertType.CONFIRMATION);
        dialog.setHeaderText("接触数据");
        dialog.setContentText("""
                此功能适用于批量修整手动维护的本地数据，若非必要不建议使用！！
                此功能不同时更新索引数据，请在修整数据后“重建索引”！

                将处理以下数据项及所有子项的数据：
                                
                """
                .concat(item.toDetail()));
        dialog.getDialogPane().setPrefWidth(800);
        dialog.initOwner(App.app().getPrimaryStage());

//        dialog.showAndWait().filter(t -> t == ButtonType.OK)
//                .ifPresent(t -> ProgressLayer.showAndWait(App.app().getPrimaryGlass(), progressLayer -> {
//                    final String msg = ItemDao.items().walk(item, itm -> {
//                        if (null == itm || null == itm.provider || itm.provider.isFolder()) return;
//                        if (null == itm.provider.getToucher()) return;
//                        try {
//                            Platform.runLater(() -> progressLayer.message.setText(itm.toPrettyPath()));
//                            itm.provider.getToucher().accept(itm);
//                        } catch (Throwable ex) {
//                            logger.warn("touch", ex);
//                        }
//                    });
//                    if (msg != null) {
//                        App.app().toastError(msg);
//                        return;
//                    }
//                    App.app().toast("已接触");
//                }));
    }
}
