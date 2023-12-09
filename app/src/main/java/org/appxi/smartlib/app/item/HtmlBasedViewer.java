package org.appxi.smartlib.app.item;

import javafx.beans.binding.Bindings;
import javafx.event.ActionEvent;
import javafx.scene.control.Button;
import javafx.scene.control.Labeled;
import javafx.scene.control.MenuItem;
import javafx.scene.control.Tooltip;
import javafx.scene.input.InputEvent;
import javafx.scene.input.KeyCode;
import javafx.scene.input.KeyCodeCombination;
import javafx.scene.input.KeyCombination;
import javafx.scene.layout.StackPane;
import org.appxi.dictionary.app.explorer.DictionaryViewer;
import org.appxi.javafx.app.DesktopApp;
import org.appxi.javafx.app.search.SearcherEvent;
import org.appxi.javafx.app.web.WebViewer;
import org.appxi.javafx.app.web.WebViewerPart;
import org.appxi.javafx.control.LookupLayer;
import org.appxi.javafx.helper.FxHelper;
import org.appxi.javafx.visual.MaterialIcon;
import org.appxi.javafx.web.WebPane;
import org.appxi.javafx.workbench.WorkbenchApp;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.javafx.workbench.WorkbenchPart;
import org.appxi.smartcn.pinyin.PinyinHelper;
import org.appxi.smartlib.ItemEvent;
import org.appxi.smartlib.app.App;
import org.appxi.smartlib.app.recent.RecentViewSupport;
import org.appxi.util.StringHelper;
import org.appxi.util.ext.Attributes;
import org.appxi.util.ext.LookupExpression;

import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Collection;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Objects;
import java.util.Optional;
import java.util.Set;
import java.util.stream.Stream;

public class HtmlBasedViewer extends WebViewerPart.MainView implements ItemRenderer, RecentViewSupport {
    public static List<String> getWebIncludeURIsEx() {
        List<String> result = getWebIncludeURIs();
        final Path dir = DesktopApp.appDir().resolve("template/web-incl");
        result.addAll(Stream.of("html-viewer.css", "html-viewer.js")
                .map(s -> dir.resolve(s).toUri().toString())
                .toList()
        );
        result.add("<link id=\"CSS\" rel=\"stylesheet\" type=\"text/css\" href=\"" + App.app().visualProvider.getWebStyleSheetURI() + "\">");
        return result;
    }

    public static void bindingViewer(WorkbenchPart.MainView viewer, ItemEx item) {
        viewer.id().bind(item.path);
        viewer.title().bind(item.name);
        viewer.tooltip().bind(Bindings.createStringBinding(item::toDetail, item.path));
        viewer.appTitle().bind(item.name);
    }

    public final ItemEx item;
    private ItemEx location;

    public HtmlBasedViewer(WorkbenchPane workbench, ItemEx item) {
        super(workbench);
        this.item = item;
        bindingViewer(this, item);
    }

    public final ItemEx item() {
        return item;
    }

    @Override
    protected final Object location() {
        return this.location;
    }

    @Override
    protected final String locationId() {
        return this.location.getPath();
    }

    @Override
    public void postConstruct() {
    }

    @Override
    public void deinitialize() {
        super.deinitialize();
        //
        app.eventBus.fireEvent(new ItemEvent(ItemEvent.VISITED, item));
    }

    @Override
    public void initialize() {
        super.initialize();
        //
        addTool_GotoHeadings();
        //
        WebViewer.addShortcutKeys(this);
        DictionaryViewer.addShortcutKeys(this);
        HtmlBasedViewer.addShortcutKeys(this);

        WebViewer.addShortcutMenu(this);
        DictionaryViewer.addShortcutMenu(this);
        HtmlBasedViewer.addShortcutMenu(this);
    }

    private Button gotoHeadings;
    private LookupLayer<String> gotoHeadingsLayer;

    protected void addTool_GotoHeadings() {
        gotoHeadings = MaterialIcon.NEAR_ME.flatButton();
        gotoHeadings.setText("转到");
        gotoHeadings.setTooltip(new Tooltip("转到 (Ctrl+T)"));
        gotoHeadings.setOnAction(event -> {
            if (null == gotoHeadingsLayer) {
                gotoHeadingsLayer = new LookupLayerImpl(viewport);
            }
            gotoHeadingsLayer.show(null);
        });
        //
        this.webPane.getTopBar().addLeft(gotoHeadings);
    }

    @Override
    protected void navigating(Object location, boolean firstTime) {
        ItemEx item = (ItemEx) location;
        final Attributes pos = null != item ? item : popPosition();
        if (null == item) {
            item = this.item;
        }

        if (null != this.location && Objects.equals(item.getPath(), this.location.getPath())) {
            this.location = item;
            setPosition(pos);
            position(pos);
            return;
        }
        this.location = item;
        setPosition(item);
        //
        super.navigating(location, firstTime);
    }

    @Override
    protected Object createWebContent() {
        return Path.of(item.getPath());
    }

    @Override
    protected void onWebEngineLoadSucceeded() {
        super.onWebEngineLoadSucceeded();
        //
        app.eventBus.fireEvent(new ItemEvent(ItemEvent.VISITED, this.item));
    }

    class LookupLayerImpl extends LookupLayer<String> {
        public LookupLayerImpl(StackPane owner) {
            super(owner);
        }

        @Override
        protected String getHeaderText() {
            return "快捷跳转章节/标题";
        }

        @Override
        protected void helpButtonAction(ActionEvent actionEvent) {
            FxHelper.showTextViewerWindow(app, "appGotoChapters.helpWindow", getHeaderText() + "使用方法",
                    """
                            >> 快捷键：Ctrl+T 在阅读视图中开启；ESC 或 点击透明区 退出此界面；上下方向键选择列表项；回车键打开；
                                    """);
        }

        private Set<String> usedKeywords;

        @Override
        protected void updateItemLabel(Labeled labeled, String data) {
            labeled.setText(data.split("#", 2)[1]);
            //
            FxHelper.highlight(labeled, usedKeywords);
        }

        @Override
        protected LookupResult<String> lookupByKeywords(String lookupText, int resultLimit) {
            final List<String> result = new ArrayList<>();
            usedKeywords = new LinkedHashSet<>();
            //
            final boolean isInputEmpty = lookupText.isBlank();
            Optional<LookupExpression> optional = isInputEmpty ? Optional.empty() : LookupExpression.of(lookupText,
                    (parent, text) -> new LookupExpression.Keyword(parent, text) {
                        @Override
                        public double score(Object data) {
                            final String text = null == data ? "" : data.toString();
                            if (this.isAsciiKeyword()) {
                                String dataInAscii = PinyinHelper.pinyin(text);
                                if (dataInAscii.contains(this.keyword())) return 1;
                            }
                            return super.score(data);
                        }
                    });
            if (!isInputEmpty && optional.isEmpty()) {
                // not a valid expression
                return new LookupResult<>(0, 0, result);
            }
            final LookupExpression lookupExpression = optional.orElse(null);
            //
            String headings = webPane.executeScript("getHeadings()");
            if (null != headings && !headings.isEmpty()) {
                headings.lines().forEach(str -> {
                    String[] arr = str.split("#", 2);
                    if (arr.length != 2 || arr[1].isBlank()) return;

                    final String hTxt = arr[1].strip();

                    double score = isInputEmpty ? 1 : lookupExpression.score(hTxt);
                    if (score > 0)
                        result.add(arr[0].concat("#").concat(hTxt));
                });
            }
            //
            if (null != lookupExpression)
                lookupExpression.keywords().forEach(k -> usedKeywords.add(k.keyword()));

            return new LookupResult<>(result.size(), result.size(), result);
        }

        @Override
        protected void lookupByCommands(String searchTerm, Collection<String> result) {
        }

        @Override
        protected void handleEnterOrDoubleClickActionOnSearchResultList(InputEvent event, String data) {
            String[] arr = data.split("#", 2);
            if (arr[0].isEmpty()) return;
            item.attr("position.selector", "#".concat(arr[0]));

            hide();
            position(item);
        }

        @Override
        public void hide() {
            super.hide();
            webPane.webView().requestFocus();
        }
    }

    ////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

    public static void addShortcutKeys(HtmlBasedViewer webViewer) {
        final WebPane webPane = webViewer.webPane;
        final WorkbenchApp app = webViewer.app;
        //
        // Ctrl + T
        webPane.shortcutKeys.put(new KeyCodeCombination(KeyCode.T, KeyCombination.SHORTCUT_DOWN), event -> {
            if (null != webViewer.gotoHeadings) {
                webViewer.gotoHeadings.fire();
                event.consume();
            }
        });
    }

    public static void addShortcutMenu(HtmlBasedViewer webViewer) {
        final WebPane webPane = webViewer.webPane;
        final WorkbenchApp app = webViewer.app;
        //
        webPane.shortcutMenu.add(selection -> {
            MenuItem copyRef = new MenuItem("复制文字（&引用出处）");
            copyRef.getProperties().put(WebPane.GRP_MENU, "copy");
            copyRef.setDisable(!selection.hasText);
            copyRef.setOnAction(event -> FxHelper.copyText("《" + webViewer.item.getName() + "》\n\n" + selection.text));
            //
            MenuItem bookmark = new MenuItem("添加书签");
            bookmark.getProperties().put(WebPane.GRP_MENU, "user1");
            bookmark.setDisable(true);

            MenuItem favorite = new MenuItem("添加收藏");
            favorite.getProperties().put(WebPane.GRP_MENU, "user1");
            favorite.setDisable(true);

            return List.of(copyRef, bookmark, favorite);
        });
        //
        webPane.shortcutMenu.add(selection -> {
            String textTip = selection.hasTrims ? "：" + StringHelper.trimChars(selection.trims, 8) : "";
            String textForSearch = selection.hasTrims ? selection.trims : null;

            MenuItem searchInBook = new MenuItem("全文检索（检索本书）".concat(textTip));
            searchInBook.getProperties().put(WebPane.GRP_MENU, "search");
            searchInBook.setOnAction(event -> app.eventBus.fireEvent(SearcherEvent.ofSearch(textForSearch, webViewer.item.parentItem())));

            return List.of(searchInBook);
        });
    }
}
