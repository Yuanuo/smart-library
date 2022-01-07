package org.appxi.smartlib.item;

import javafx.application.Platform;
import javafx.scene.control.TextInputDialog;
import org.apache.commons.io.FilenameUtils;
import org.appxi.holder.BoolHolder;
import org.appxi.search.solr.Piece;
import org.appxi.smartlib.App;
import org.appxi.smartlib.AppContext;
import org.appxi.smartlib.dao.DataApi;
import org.appxi.util.DigestHelper;
import org.appxi.util.FileHelper;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.function.Consumer;
import java.util.function.Function;
import java.util.stream.Collectors;

public abstract class AbstractProvider implements ItemProvider {
    private Consumer<Item> creator;

    @Override
    public Consumer<Item> getCreator() {
        if (null != this.creator) return this.creator;
        return this.creator = parent -> {
            final TextInputDialog dialog = new TextInputDialog(parent.isRoot() ? "/" : "/".concat(parent.getPath()).concat("/"));
            dialog.setTitle("添加 ".concat(providerName()));
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
                Item item = new Item(this);
                item.setName(FilenameUtils.getName(str));
                item.setPath(str);
                //
                final String msg = DataApi.dataAccess().create(item);
                if (null != msg) {
                    AppContext.toastError(msg);
                    return;
                }
                App.app().eventBus.fireEvent(new ItemEvent(ItemEvent.CREATED, item));
            });
        };
    }

    private Function<Item, List<Piece>> indexer;

    @Override
    public Function<Item, List<Piece>> getIndexer() {
        if (null != this.indexer) return this.indexer;
        return this.indexer = item -> {
            final Piece mainPiece = Piece.of();
            mainPiece.provider = providerId();
            mainPiece.path = item.getPath();
            mainPiece.type = "location";
            mainPiece.title = item.getName();
            //
            Piece piece = mainPiece.clone();
            piece.id = DigestHelper.uid();
            piece.title = mainPiece.title;
            piece.field("title_txt_aio", piece.title);
            piece.field("title_txt_en", AppContext.ascii(piece.title));
            //
            final List<Piece> result = new ArrayList<>();
            result.add(piece);
            return result;
        };
    }
}
