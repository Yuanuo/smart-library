package org.appxi.smartlib.explorer;

import javafx.application.Platform;
import javafx.scene.control.Alert;
import javafx.scene.control.ButtonType;
import javafx.scene.control.DialogPane;
import javafx.scene.control.TextArea;
import javafx.scene.control.TextInputDialog;
import javafx.stage.FileChooser;
import org.appxi.javafx.control.ProgressLayer;
import org.appxi.javafx.helper.FxHelper;
import org.appxi.prefs.UserPrefs;
import org.appxi.smartlib.App;
import org.appxi.smartlib.AppContext;
import org.appxi.smartlib.dao.DataApi;
import org.appxi.smartlib.dao.PiecesRepository;
import org.appxi.smartlib.item.Item;
import org.appxi.smartlib.item.ItemEvent;
import org.appxi.util.DateHelper;
import org.appxi.util.FileHelper;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.awt.Desktop;
import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.util.Date;
import java.util.HashSet;
import java.util.List;
import java.util.Set;
import java.util.zip.ZipEntry;
import java.util.zip.ZipFile;
import java.util.zip.ZipOutputStream;

public class ItemActions {
    private static final Logger logger = LoggerFactory.getLogger(ItemActions.class);

    static void rename(Item item) {
        final TextInputDialog dialog = new TextInputDialog(item.getName());
        dialog.setTitle("重命名");
        dialog.setHeaderText(item.typedPath());
        dialog.getDialogPane().setPrefWidth(800);
        dialog.initOwner(App.app().getPrimaryStage());
        dialog.showAndWait().ifPresent(str -> {
            str = FileHelper.toValidName(str.replaceAll("^[.]", "")).strip();
            if (str.isEmpty() || str.equals(item.getName()))
                return;
            final String newName = str;
            ProgressLayer.showAndWait(App.app().getPrimaryGlass(), progressLayer -> {
                final Item oldItem = item.clone(), newItem = item.clone();
                final String msg = DataApi.dataAccess().rename(newItem, newName);
                if (msg != null) {
                    App.app().toastError(msg);
                    return;
                }
                // update indexes
                final PiecesRepository repository = AppContext.getBean(PiecesRepository.class);
                if (null != repository) {
                    repository.deleteAllByPath(item.getPath());
                    DataApi.dataAccess().reindex(newItem, (d, s) -> Platform.runLater(() -> progressLayer.message.setText(s)));
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
        dialog.setContentText("将删除：\n".concat(item.typedPath()));
        dialog.initOwner(App.app().getPrimaryStage());
        dialog.getDialogPane().setPrefWidth(800);
        dialog.showAndWait().filter(t -> t == ButtonType.OK).ifPresent(t -> ProgressLayer.showAndWait(App.app().getPrimaryGlass(), progressLayer -> {
            final String msg = DataApi.dataAccess().delete(item, (d, s) -> Platform.runLater(() -> progressLayer.message.setText(s)));
            if (msg != null) {
                App.app().toastError(msg);
                return;
            }
            // update indexes
            final PiecesRepository repository = AppContext.getBean(PiecesRepository.class);
            if (null != repository) {
                if (item.provider.isDirectory()) {
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
            final String msg = DataApi.dataAccess().move(newItem, newParent);
            if (msg != null) {
                App.app().toastError(msg);
                return;
            }

            // update indexes
            final PiecesRepository repository = AppContext.getBean(PiecesRepository.class);
            if (null != repository) {
                repository.deleteAllByPath(item.getPath());
                DataApi.dataAccess().reindex(newItem, (d, s) -> Platform.runLater(() -> progressLayer.message.setText(s)));
            }

            FxHelper.runLater(() -> {
                item.setName(newItem.getName()).setPath(newItem.getPath());
                //
                App.app().eventBus.fireEvent(new ItemEvent(ItemEvent.MOVED, item, oldItem));
            });
        });

        List<String> exists = DataApi.dataAccess().exists(item.getPath(), newParent.getPath());
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
                .replace("${source}", item.typedPath())
                .replace("${target}", newParent.typedPath()));
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
                .concat(item.typedPath()));
        dialog.getDialogPane().setPrefWidth(800);
        dialog.initOwner(App.app().getPrimaryStage());
        dialog.showAndWait().filter(t -> t == ButtonType.OK)
                .ifPresent(t -> ProgressLayer.showAndWait(App.app().getPrimaryGlass(), progressLayer -> {
                    final String msg = DataApi.dataAccess().reindex(item, (d, s) -> Platform.runLater(() -> progressLayer.message.setText(s)));
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
                final String msg = DataApi.dataAccess().backup(item, zipOutput, (d, s) -> Platform.runLater(() -> progressLayer.message.setText(s)));
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
            final Set<String> rootPaths = new HashSet<>();
            // unpack zip files
            for (File file : files) {
                try (ZipFile zipFile = new ZipFile(file)) {
                    final String msg = DataApi.dataAccess().restore(item, zipFile, (d, s) -> Platform.runLater(() -> progressLayer.message.setText(s)));
                    if (null != msg) {
                        App.app().toastError(msg);
                    }
                    rootPaths.addAll(zipFile.stream().filter(zipEntry -> !zipEntry.getName().contains("/"))
                            .map(ZipEntry::getName)
                            .toList());
                } catch (Exception e) {
                    logger.error("restore", e);
                    App.app().toastError(e.getMessage());
                }
            }
            // 更新索引，避免重建父级全目录耗时太长
            for (String rootPath : rootPaths) {
                Item rootItem = DataApi.dataAccess().resolve(item.getPath() + "/" + rootPath);
                DataApi.dataAccess().reindex(rootItem, (d, s) -> Platform.runLater(() -> progressLayer.message.setText(s)));
            }

            App.app().toast("已导入");
            App.app().eventBus.fireEvent(new ItemEvent(ItemEvent.RESTORED, item));
        }));
    }

    public static void setContent(Item item, InputStream content, boolean reindex) {
        final String msg = DataApi.dataAccess().setContent(item, content);
        if (msg != null) {
            App.app().toastError(msg);
            return;
        }
        // update indexes
        if (reindex)
            DataApi.dataAccess().reindex(item, (d, s) -> {
            });
    }

    public static void touch(Item item) {
        final Alert dialog = new Alert(Alert.AlertType.CONFIRMATION);
        dialog.setHeaderText("接触数据");
        dialog.setContentText("""
                此功能适用于批量修整手动维护的本地数据，若非必要不建议使用！！
                此功能不同时更新索引数据，请在修整数据后“重建索引”！

                将处理以下数据项及所有子项的数据：
                                
                """
                .concat(item.typedPath()));
        dialog.getDialogPane().setPrefWidth(800);
        dialog.initOwner(App.app().getPrimaryStage());

        dialog.showAndWait().filter(t -> t == ButtonType.OK)
                .ifPresent(t -> ProgressLayer.showAndWait(App.app().getPrimaryGlass(), progressLayer -> {
                    final String msg = DataApi.dataAccess().walk(item, itm -> {
                        if (null == itm || null == itm.provider || itm.provider.isDirectory()) return;
                        if (null == itm.provider.getToucher()) return;
                        try {
                            Platform.runLater(() -> progressLayer.message.setText(itm.typedPath()));
                            itm.provider.getToucher().accept(itm);
                        } catch (Throwable ex) {
                            logger.warn("touch", ex);
                        }
                    });
                    if (msg != null) {
                        App.app().toastError(msg);
                        return;
                    }
                    App.app().toast("已接触");
                }));
    }
}
