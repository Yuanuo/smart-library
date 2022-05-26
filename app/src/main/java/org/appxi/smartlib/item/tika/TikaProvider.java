package org.appxi.smartlib.item.tika;

import javafx.scene.control.TreeItem;
import org.apache.tika.Tika;
import org.apache.tika.metadata.Metadata;
import org.appxi.javafx.visual.MaterialIcon;
import org.appxi.prefs.UserPrefs;
import org.appxi.search.solr.Piece;
import org.appxi.smartlib.App;
import org.appxi.smartlib.AppContext;
import org.appxi.smartlib.dao.DataApi;
import org.appxi.smartlib.html.HtmlRepairer;
import org.appxi.smartlib.item.AbstractProvider;
import org.appxi.smartlib.item.Item;
import org.appxi.smartlib.item.ItemRenderer;
import org.appxi.util.DigestHelper;
import org.appxi.util.FileHelper;
import org.appxi.util.StringHelper;
import org.jsoup.Jsoup;
import org.jsoup.nodes.Document;
import org.jsoup.nodes.Element;
import org.jsoup.nodes.Node;

import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.function.Consumer;
import java.util.function.Function;
import java.util.function.Supplier;
import java.util.stream.Collectors;

abstract class TikaProvider extends AbstractProvider {
    @Override
    public final boolean isDirectory() {
        return false;
    }

    @Override
    public javafx.scene.Node getItemIcon(TreeItem<Item> treeItem) {
        return MaterialIcon.PLAGIARISM.graphic();
    }

    @Override
    public final Consumer<Item> getCreator() {
        return null;
    }

    private Function<Item, ItemRenderer> viewer;

    @Override
    public Function<Item, ItemRenderer> getViewer() {
        if (null != this.viewer) return this.viewer;
        return this.viewer = item -> new TikaViewer(item, App.app().workbench());
    }

    private Function<Item, List<Piece>> indexer;

    @Override
    public Function<Item, List<Piece>> getIndexer() {
        if (null != this.indexer) return this.indexer;

        return this.indexer = item -> {
            // prepare common parts
            final Piece mainPiece = Piece.of();
            mainPiece.provider = providerId();
            mainPiece.path = item.getPath();
            mainPiece.type = "article";
            mainPiece.title = item.getName();
            //
            Piece piece = mainPiece.clone();
            piece.id = DigestHelper.uid62s();
            piece.title = mainPiece.title;
            piece.field("title_txt_aio", piece.title);
            piece.field("title_txt_en", AppContext.ascii(piece.title));
            piece.text("text_txt_aio", toExtractedText(item));
            //
            final List<Piece> result = new ArrayList<>();
            result.add(piece);
            return result;
        };
    }

    ////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
    public final String toExtractedText(Item item) {
        try (InputStream stream = DataApi.dataAccess().getContent(item)) {
            Metadata metadata = new Metadata();
            return new Tika().parseToString(stream, metadata, -1);
        } catch (Throwable e) {
            e.printStackTrace();
        }
        return "无内容";
    }

    public String toViewableHtmlDocument(Item item, Supplier<Element> bodySupplier, Function<Element, Object> bodyWrapper, String... includes) {
        Element body = null != bodySupplier ? bodySupplier.get() : null;
        if (null == body) {
            Document document = Jsoup.parse("");
            body = document.body();
            String extractedText = this.toExtractedText(item);
            extractedText = extractedText.lines().map(s -> "<p>".concat(s).concat("</p>")).collect(Collectors.joining());
            body.html(extractedText);
        }
        // fill id for elements
        body.traverse(new HtmlRepairer(1).withMost());
        //
        final StringBuilder buff = new StringBuilder();
        buff.append("<!DOCTYPE html><html lang=\"");
//        buff.append(body == body() ? getTextLocale().lang : "en");
        buff.append(Locale.getDefault().getLanguage());
        buff.append("\"><head><meta charset=\"UTF-8\">");
        //
        final List<String> scripts = new ArrayList<>(), styles = new ArrayList<>();
        for (String include : includes) {
            if (include.endsWith(".js")) {
                buff.append("\r\n<script type=\"text/javascript\" src=\"").append(include).append("\"></script>");
            } else if (include.endsWith(".css")) {
                buff.append("\r\n<link rel=\"stylesheet\" href=\"").append(include).append("\"/>");
            } else if (include.startsWith("<script") || include.startsWith("<style")
                    || include.startsWith("<link") || include.startsWith("<meta")) {
                buff.append("\r\n").append(include);
            } else if (include.startsWith("var ") || include.startsWith("function")) {
                scripts.add(include);
            } else {
                styles.add(include);
            }
        }
        if (!scripts.isEmpty()) {
            buff.append("\r\n<script type=\"text/javascript\">").append(StringHelper.joinLines(scripts)).append("</script>");
        }
        if (!styles.isEmpty()) {
            buff.append("\r\n<style type=\"text/css\">").append(StringHelper.joinLines(styles)).append("</style>");
        }
        //
        buff.append("</head>");
        if (null == bodyWrapper) {
            buff.append(body.outerHtml());
        } else {
            final Object bodyWrapped = bodyWrapper.apply(body);
            if (bodyWrapped instanceof Node node) {
                buff.append(node.outerHtml());
            } else {
                final String bodyHtml = bodyWrapped.toString();
                if (bodyHtml.startsWith("<body"))
                    buff.append(bodyHtml);
                else buff.append("<body>").append(bodyHtml).append("</body>");
            }
        }
        buff.append("</html>");
        return buff.toString();
    }

    private static final String VERSION = "22.01.10";

    public String toViewableHtmlFile(Item item, Supplier<Element> bodySupplier, Function<Element, Object> bodyWrapper, String... includes) {
        final StringBuilder cacheInfo = new StringBuilder();
        cacheInfo.append(DataApi.dataAccess().getIdentificationInfo(item));
        cacheInfo.append(VERSION);
//        cacheInfo.append(hanLang.lang);
        cacheInfo.append(StringHelper.join("|", includes));
        final String cachePath = StringHelper.concat(".tmp.", DigestHelper.md5(cacheInfo.toString()), ".html");
        final Path cacheFile = UserPrefs.dataDir().resolve(".temp").resolve(cachePath);
        if (Files.notExists(cacheFile)) {
            final String stdHtmlDoc = this.toViewableHtmlDocument(item, bodySupplier, bodyWrapper, includes);
            final boolean success = FileHelper.writeString(stdHtmlDoc, cacheFile);
            if (success) {
//                DevtoolHelper.LOG.info("Cached : " + cacheFile.toAbsolutePath());
            } else throw new RuntimeException("cannot cache stdHtmlDoc");// for debug only
        }
        return cacheFile.toAbsolutePath().toString();
    }
}
