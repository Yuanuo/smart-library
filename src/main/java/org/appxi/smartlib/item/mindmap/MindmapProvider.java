package org.appxi.smartlib.item.mindmap;

import javafx.scene.Node;
import javafx.scene.control.TreeItem;
import org.appxi.javafx.visual.MaterialIcon;
import org.appxi.search.solr.Piece;
import org.appxi.smartlib.App;
import org.appxi.smartlib.AppContext;
import org.appxi.smartlib.item.AbstractProvider;
import org.appxi.smartlib.item.Item;
import org.appxi.smartlib.item.ItemHelper;
import org.appxi.smartlib.item.ItemRenderer;
import org.appxi.smartlib.search.Searchable;
import org.appxi.util.DigestHelper;
import org.appxi.util.NumberHelper;
import org.appxi.util.StringHelper;

import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.function.Consumer;
import java.util.function.Function;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import java.util.stream.Collectors;

public class MindmapProvider extends AbstractProvider {
    public static final MindmapProvider ONE = new MindmapProvider();
    public static final Pattern P_INDEXED_NAME = Pattern.compile("(\\d+)(\\.\s+)(.*)");

    private MindmapProvider() {
    }

    @Override
    public String providerId() {
        return "km.json";
    }

    @Override
    public String providerName() {
        return "脑图";
    }

    @Override
    public final boolean isDirectory() {
        return false;
    }

    @Override
    public Node getItemIcon(TreeItem<Item> treeItem) {
        return MaterialIcon.ACCOUNT_TREE.graphic();
    }

    @Override
    public String getItemName(String name) {
        return ItemHelper.nameWithoutProvider(name, providerId());
    }

    private Function<Item, ItemRenderer> editor;

    @Override
    public Function<Item, ItemRenderer> getEditor() {
        if (null != this.editor) return this.editor;
        return this.editor = item -> new MindmapEditor(item, App.app().workbench());
    }

    private Function<Item, ItemRenderer> viewer;

    @Override
    public Function<Item, ItemRenderer> getViewer() {
        if (null != this.viewer) return this.viewer;
        return this.viewer = item -> new MindmapViewer(item, App.app().workbench());
    }

    private Function<Item, List<Piece>> indexer;

    @Override
    public Function<Item, List<Piece>> getIndexer() {
        if (null != this.indexer) return this.indexer;
        return this.indexer = item -> {
            final MindmapDocument mainDocument = new MindmapDocument(item);
            final Searchable searchable = mainDocument.getSearchable();
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
            List<String> metaList = mainDocument.getMetadata("library");
            if (metaList.isEmpty()) addCategories(mainPiece, "library", "unknown");
            else metaList.forEach(v -> addCategories(mainPiece, "library", v));
            //
            metaList = mainDocument.getMetadata("catalog");
            if (metaList.isEmpty()) addCategories(mainPiece, "catalog", "unknown");
            else metaList.forEach(v -> addCategories(mainPiece, "catalog", v));
            //
            metaList = mainDocument.getMetadata("period");
            metaList.addAll(mainDocument.getTagged("period").values()); // detect periods from tagged
            if (metaList.isEmpty()) addCategories(mainPiece, "period", "unknown");
            else metaList.forEach(v -> addCategories(mainPiece, "period", v));
            //
            metaList = mainDocument.getMetadata("author");
            metaList.addAll(mainDocument.getTagged("author", "translator").values()); // detect authors from tagged
            if (metaList.isEmpty()) {
                addCategories(mainPiece, "author", "unknown");
                mainPiece.field("authors_s", "unknown");
            } else {
                metaList.forEach(v -> addCategories(mainPiece, "author", v));
                mainPiece.field("authors_s", metaList.stream().map(v -> v.split("[\s　]")[0])
                        .collect(Collectors.joining("; ")));
            }
            //
            Optional.ofNullable(mainDocument.getMetadata("priority", null))
                    .ifPresent(v -> mainPiece.priority = NumberHelper.toDouble(v, 5));
            Optional.ofNullable(mainDocument.getMetadata("sequence", sequence))
                    .ifPresent(v -> mainPiece.field("sequence_s", v));
            //
            final List<Piece> result = new ArrayList<>();
            final List<MindmapDocument> documents = mainDocument.toSearchableDocuments();
            final boolean articleDocumentOnly = documents.size() == 1 && documents.get(0) == mainDocument;
            //
            for (int j = 0; j < documents.size(); j++) {
                MindmapDocument document = documents.get(j);
                Piece piece = mainPiece.clone();
                // detect topic title
                Map<String, String> headings = document.getTagged("heading", "alias");
                if (articleDocumentOnly) {
                    piece.id = mainDocument.id();
                    piece.title = mainPiece.title;
                } else {
                    piece.id = DigestHelper.uid62s();
                    piece.type = "topic";
                    piece.title = "<alias but no HEADING>";
                    if (!headings.isEmpty()) {
                        String id = headings.keySet().iterator().next();
                        piece.title = headings.get(id);
                        piece.field("anchor_s", id);
                    }
                    if (j == 0) {
                        if (mainPiece.title.startsWith(piece.title) || mainPiece.title.endsWith(piece.title))
                            piece.setTitle(mainPiece.title).setType("article");
                        else if (piece.title.startsWith(mainPiece.title) || piece.title.endsWith(mainPiece.title))
                            piece.setType("article");
                        else result.add(createPiece(mainPiece.path, null, mainPiece.title, "topic"));
                    }
                }
                //
                piece.field("title_txt_aio", piece.title);
                piece.field("title_txt_en", AppContext.ascii(piece.title));

                if (searchable == Searchable.all) {
                    String documentText = document.getDocumentText();
                    if (null != documentText && !documentText.isBlank())
                        piece.text("text_txt_aio", documentText);
                }
                //
                //
                result.add(piece);

                int i = -1;
                for (Map.Entry<String, String> head : headings.entrySet()) {
                    i++;
                    String headText = head.getValue().strip();
                    if (headText.isBlank()) continue;
                    if (i == 0 && (piece.title.endsWith(headText) || headText.endsWith(piece.title))) continue;
                    result.add(createPiece(piece.path, head.getKey(), headText, "label"));
                }
            }

            return result;
        };
    }

    private Piece createPiece(String path, String anchor, String title, String type) {
        Piece piece = Piece.of();
        piece.provider = providerId();
        piece.id = DigestHelper.uid62s();
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

    private Consumer<Item> toucher;

    @Override
    public Consumer<Item> getToucher() {
        if (null != this.toucher) return this.toucher;
        return this.toucher = item -> {
            MindmapDocument document = new MindmapDocument(item);
            //
            if (document.getMetadata("library").isEmpty()) document.setMetadata("library", "unknown");
            //
            if (document.getMetadata("catalog").isEmpty()) document.setMetadata("catalog", "unknown");
            //
            if (document.getMetadata("period").isEmpty()) document.setMetadata("period", "unknown");
            //
            if (document.getMetadata("author").isEmpty()) document.setMetadata("author", "unknown");

            document.save(false);
        };
    }
}
