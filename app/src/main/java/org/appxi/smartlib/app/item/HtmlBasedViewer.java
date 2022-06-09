package org.appxi.smartlib.app.item;

import javafx.scene.control.Button;
import javafx.scene.control.ContextMenu;
import javafx.scene.control.Labeled;
import javafx.scene.control.MenuItem;
import javafx.scene.control.SeparatorMenuItem;
import javafx.scene.control.Tooltip;
import javafx.scene.input.Clipboard;
import javafx.scene.input.DataFormat;
import javafx.scene.input.InputEvent;
import javafx.scene.input.KeyCode;
import javafx.scene.input.KeyEvent;
import javafx.scene.layout.StackPane;
import org.appxi.javafx.app.search.SearcherEvent;
import org.appxi.javafx.app.web.WebCallback;
import org.appxi.javafx.app.web.WebRenderer;
import org.appxi.javafx.app.web.WebViewerPart;
import org.appxi.javafx.control.LookupLayer;
import org.appxi.javafx.helper.FxHelper;
import org.appxi.javafx.visual.MaterialIcon;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.smartcn.pinyin.PinyinHelper;
import org.appxi.smartlib.ItemEvent;
import org.appxi.smartlib.app.AppContext;
import org.appxi.smartlib.app.recent.RecentViewSupport;
import org.appxi.util.StringHelper;
import org.appxi.util.ext.Attributes;
import org.appxi.util.ext.LookupExpression;

import java.io.InputStream;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Collection;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.Optional;
import java.util.Set;

public class HtmlBasedViewer extends WebViewerPart.MainView implements ItemRenderer, RecentViewSupport {
    public final ItemEx item;
    private ItemEx location;

    public HtmlBasedViewer(WorkbenchPane workbench, StackPane viewport, ItemEx item) {
        super(workbench, viewport);
        this.item = item;
        AppContext.bindingViewer(this, item);
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
    protected List<InputStream> getAdditionalStyleSheets() {
        return AppContext.getWebIncludeCSSs();
    }

    @Override
    public void initialize() {
    }

    @Override
    public void uninstall() {
        super.uninstall();
        //
        app.eventBus.fireEvent(new ItemEvent(ItemEvent.VISITED, item));
    }

    @Override
    public void install() {
        super.install();
        //
        addTool_GotoHeadings();
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
        webPane().getTopBar().addLeft(gotoHeadings);
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
    protected WebCallback createWebCallback() {
        return new WebCallbackImpl(this);
    }

    @Override
    protected void onWebEngineLoadSucceeded() {
        super.onWebEngineLoadSucceeded();
        //
        app.eventBus.fireEvent(new ItemEvent(ItemEvent.VISITED, this.item));
    }

    @Override
    protected void handleWebViewShortcuts(KeyEvent event) {
        if (!event.isConsumed() && event.isShortcutDown()) {
            // Ctrl + T
            if (event.getCode() == KeyCode.T && null != gotoHeadings) {
                gotoHeadings.fire();
                event.consume();
                return;
            }
        }
        super.handleWebViewShortcuts(event);
    }

    @Override
    protected ContextMenu createWebViewContextMenu() {
        String origText = webPane().executeScript("getValidSelectionText()");
        String trimText = null == origText ? null : origText.strip().replace('\n', ' ');
        final String availText = StringHelper.isBlank(trimText) ? null : trimText;
        //
        MenuItem copyRef = new MenuItem("复制引用");
        copyRef.setDisable(null == availText);
        copyRef.setOnAction(event -> Clipboard.getSystemClipboard().setContent(Map.of(DataFormat.PLAIN_TEXT,
                "《".concat(item.getName()).concat("》\n\n").concat(origText))));

        //
        String textTip = null == availText ? "" : "：".concat(StringHelper.trimChars(availText, 8));

        MenuItem searchInBook = new MenuItem("全文检索（检索本书）".concat(textTip));
        searchInBook.setOnAction(event -> app.eventBus.fireEvent(SearcherEvent.ofSearch(availText, item.parentItem())));

        //
        MenuItem bookmark = new MenuItem("添加书签");
        bookmark.setDisable(true);

        MenuItem favorite = new MenuItem("添加收藏");
        favorite.setDisable(true);

        //
        return new ContextMenu(
                createMenu_copy(origText, availText),
                copyRef,
                new SeparatorMenuItem(),
                createMenu_search(textTip, availText),
                createMenu_searchExact(textTip, availText),
                searchInBook,
                createMenu_lookup(textTip, availText),
                createMenu_finder(textTip, availText),
                new SeparatorMenuItem(),
                createMenu_dict(availText),
                createMenu_pinyin(availText),
                new SeparatorMenuItem(),
                bookmark,
                favorite
        );
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
        protected String getUsagesText() {
            return """
                    >> 快捷键：Ctrl+T 在阅读视图中开启；ESC 或 点击透明区 退出此界面；上下方向键选择列表项；回车键打开；
                    """;
        }

        private Set<String> usedKeywords;

        @Override
        protected void updateItemLabel(Labeled labeled, String data) {
            labeled.setText(data.split("#", 2)[1]);
            //
            FxHelper.highlight(labeled, usedKeywords);
        }

        @Override
        protected Collection<String> lookupByKeywords(String lookupText, int resultLimit) {
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
                return result;
            }
            final LookupExpression lookupExpression = optional.orElse(null);
            //
            String headings = webPane().executeScript("getHeadings()");
            if (null != headings && headings.length() > 0) {
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
            return result;
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
            webPane().webView().requestFocus();
        }
    }

    public class WebCallbackImpl extends WebCallback {
        public WebCallbackImpl(WebRenderer webRenderer) {
            super(webRenderer);
        }
    }
}
