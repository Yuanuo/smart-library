package org.appxi.smartlib.item.article;

import javafx.scene.Node;
import javafx.scene.control.TreeItem;
import org.appxi.javafx.visual.MaterialIcon;
import org.appxi.prefs.UserPrefs;
import org.appxi.search.solr.Piece;
import org.appxi.smartlib.App;
import org.appxi.smartlib.AppContext;
import org.appxi.smartlib.item.AbstractProvider;
import org.appxi.smartlib.item.Item;
import org.appxi.smartlib.item.ItemEditor;
import org.appxi.smartlib.item.ItemHelper;
import org.appxi.smartlib.item.ItemViewer;
import org.appxi.smartlib.search.Searchable;
import org.appxi.util.DigestHelper;
import org.appxi.util.NumberHelper;
import org.appxi.util.StringHelper;
import org.jsoup.nodes.Element;
import org.jsoup.select.Elements;

import java.util.ArrayList;
import java.util.List;
import java.util.Optional;
import java.util.function.Function;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import java.util.stream.Collectors;

public class ArticleProvider extends AbstractProvider {
    public static final ArticleProvider ONE = new ArticleProvider();
    public static final Pattern P_INDEXED_NAME = Pattern.compile("(\\d+)(\\.\s+)(.*)");

    private ArticleProvider() {
    }

    @Override
    public String providerId() {
        return "html";
    }

    @Override
    public String providerName() {
        return "图文";
    }

    @Override
    public final boolean isDirectory() {
        return false;
    }

    @Override
    public Node getItemIcon(TreeItem<Item> treeItem) {
        return null == treeItem ? MaterialIcon.POST_ADD.graphic() : MaterialIcon.TEXT_SNIPPET.graphic();
    }

    @Override
    public String getItemName(String name) {
        return ItemHelper.nameWithoutProvider(name, providerId());
    }

    private Function<Item, ItemEditor> editor;

    @Override
    public Function<Item, ItemEditor> getEditor() {
        if (null != this.editor) return this.editor;
        return this.editor = item ->
                "simple".equals(UserPrefs.prefs.getString("item.article.editor", "advanced"))
                        ? new ArticleEditorOld(item, App.app().workbench())
                        : new ArticleEditor(item, App.app().workbench());
    }

    private Function<Item, ItemViewer> viewer;

    @Override
    public Function<Item, ItemViewer> getViewer() {
        if (null != this.viewer) return this.viewer;
        return this.viewer = item -> new ArticleViewer(item, App.app().workbench());
    }

//    private BiConsumer<Item, LibraryTreeView> itemActionForTouch;
//    public BiConsumer<Item, LibraryTreeView> getItemActionForTouch() {
//        if (null != this.itemActionForTouch)
//            return this.itemActionForTouch;
//
//        return this.itemActionForTouch = (item, treeView) -> {
//            if (null == treeView) {
//                ArticleDocument document = new ArticleDocument(item);
//                document.getDocument();
//                document.save();
//                return;
//            }
//            final Alert alert = new Alert(Alert.AlertType.CONFIRMATION);
//            alert.setHeaderText("接触");
//            alert.setContentText("""
//                    接触：
//
//                    """
//                    .concat(item.toStringWithType()));
//            alert.initOwner(App.app().getPrimaryStage());
//            Optional<ButtonType> optional = alert.showAndWait().filter(t -> t == ButtonType.OK);
//            if (optional.isEmpty()) return;
//
//            AppContext.runBlocking(() -> {
//                try {
//                    ArticleDocument document = new ArticleDocument(item);
//                    document.getDocument();
//                    document.save();
//                } catch (Throwable t) {
//                    AppContext.toastError(t.getMessage());
//                    return;
//                }
//                AppContext.toast("已接触");
//            });
//        };
//    }

    private Function<Item, List<Piece>> indexer;

    @Override
    public Function<Item, List<Piece>> getIndexer() {
        if (null != this.indexer) return this.indexer;
        return this.indexer = item -> {
            final ArticleDocument articleDocument = new ArticleDocument(item);
            final Searchable searchable = articleDocument.getSearchable();
            if (searchable == Searchable.exclude) return null;
            // prepare common parts
            final Piece mainPiece = Piece.of();
            mainPiece.provider = providerId();
            mainPiece.path = item.getPath();
            mainPiece.type = "article";
            //
            String sequence = null;
            Matcher matcher = P_INDEXED_NAME.matcher(item.getName());
            if (matcher.matches()) {
                sequence = matcher.group(1);
                mainPiece.title = matcher.group(3);
            } else mainPiece.title = item.getName();
            //
            List<String> metaList = articleDocument.getMetadata("library");
            if (metaList.isEmpty()) addCategories(mainPiece, "library", "unknown");
            else metaList.forEach(v -> addCategories(mainPiece, "library", v));
            //
            metaList = articleDocument.getMetadata("catalog");
            if (metaList.isEmpty()) addCategories(mainPiece, "catalog", "unknown");
            else metaList.forEach(v -> addCategories(mainPiece, "catalog", v));
            //
            metaList = articleDocument.getMetadata("period");
            if (metaList.isEmpty()) addCategories(mainPiece, "period", "unknown");
            else metaList.forEach(v -> addCategories(mainPiece, "period", v));
            //
            metaList = articleDocument.getMetadata("author");
            if (metaList.isEmpty()) {
                addCategories(mainPiece, "author", "unknown");
                mainPiece.field("authors_s", "unknown");
            } else {
                metaList.forEach(v -> addCategories(mainPiece, "author", v));
                mainPiece.field("authors_s", metaList.stream().map(v -> v.split("[\s　]")[0])
                        .collect(Collectors.joining("; ")));
            }
            //
            Optional.ofNullable(articleDocument.getMetadata("priority", null))
                    .ifPresent(v -> mainPiece.priority = NumberHelper.toDouble(v, 5));
            Optional.ofNullable(articleDocument.getMetadata("sequence", sequence))
                    .ifPresent(v -> mainPiece.field("sequence_s", v));
            //
            final List<Piece> result = new ArrayList<>();
            final List<ArticleDocument> documents = articleDocument.toSearchableDocuments();
            final boolean articleDocumentOnly = documents.size() == 1 && documents.get(0) == articleDocument;
            //
            for (int j = 0; j < documents.size(); j++) {
                ArticleDocument document = documents.get(j);
                Piece piece = mainPiece.clone();
                piece.id = DigestHelper.uid();
                // detect topic title
                final Elements headings = document.body().select("h1, h2, h3, h4, h5, h6");
                if (articleDocumentOnly) {
                    piece.title = mainPiece.title;
                } else {
                    piece.type = "topic";
                    piece.title = "<HR but no HEADING>";
                    Optional.ofNullable(headings.first())
                            .ifPresent(h -> piece.setTitle(h.text().strip()).field("anchor_s", h.id()));
                    if (j == 0) {
                        if (mainPiece.title.endsWith(piece.title))
                            piece.setTitle(mainPiece.title).setType("article");
                        else if (piece.title.endsWith(mainPiece.title))
                            piece.setType("article");
                        else result.add(createPiece(mainPiece.path, null, mainPiece.title, "topic"));
                    }
                }
                //
                piece.field("title_txt_aio", piece.title);
                piece.field("title_txt_en", AppContext.ascii(piece.title));

                if (searchable == Searchable.all) {
                    piece.text("text_txt_aio", document.body().text());
                }
                //
                //
                result.add(piece);

                for (int i = 0; i < headings.size(); i++) {
                    Element head = headings.get(i);
                    String headText = head.text().strip();
                    if (headText.isBlank()) continue;
                    if (i == 0 && (piece.title.endsWith(headText) || headText.endsWith(piece.title))) continue;
                    result.add(createPiece(piece.path, head.id(), headText, "label"));
                }
            }

            return result;
        };
    }

    private Piece createPiece(String path, String anchor, String title, String type) {
        Piece piece = Piece.of();
        piece.provider = providerId();
        piece.id = DigestHelper.uid();
        piece.type = type;
        piece.path = path;
        if (null != anchor) {
            piece.field("anchor_s", anchor);
        }
        piece.title = title;
        piece.field("title_txt_aio", title);
        piece.field("title_txt_en", AppContext.ascii(title));
        return piece;
    }

    private static void addCategories(Piece piece, String group, String path) {
        if (StringHelper.isBlank(path))
            return;
        final String[] names = path.replace("//", "/").split("/");
        final List<String> paths = StringHelper.getFlatPaths(StringHelper.join("/", names));
        final String grouped = StringHelper.concat(".categories/", group, "/");
        paths.forEach(s -> piece.categories.add(grouped.concat(s)));
    }
}
