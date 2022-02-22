package org.appxi.smartlib.html;

import javafx.beans.InvalidationListener;
import javafx.beans.Observable;
import javafx.event.Event;
import javafx.print.PageLayout;
import javafx.print.PageOrientation;
import javafx.print.Paper;
import javafx.print.Printer;
import javafx.print.PrinterJob;
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
import netscape.javascript.JSObject;
import org.appxi.javafx.app.DesktopApp;
import org.appxi.javafx.control.LookupLayer;
import org.appxi.javafx.control.ProgressLayer;
import org.appxi.javafx.control.WebPane;
import org.appxi.javafx.helper.FxHelper;
import org.appxi.javafx.visual.MaterialIcon;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.prefs.UserPrefs;
import org.appxi.smartlib.AppContext;
import org.appxi.smartlib.event.SearcherEvent;
import org.appxi.smartlib.item.Item;
import org.appxi.smartlib.item.ItemEvent;
import org.appxi.smartlib.recent.RecentViewSupport;
import org.appxi.util.StringHelper;
import org.appxi.util.ext.Attributes;
import org.appxi.util.ext.LookupExpression;

import java.net.URI;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Collection;
import java.util.Comparator;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.Optional;
import java.util.Set;

public class HtmlViewer extends HtmlRenderer implements RecentViewSupport {
    private final InvalidationListener onWebViewBodyResize = this::onWebViewBodyResize;

    protected WebPane.WebMarksFinder webFinder;

    public HtmlViewer(Item item, WorkbenchPane workbench) {
        super(item, workbench, false);
    }

    @Override
    protected void onViewportInitOnce(StackPane viewport) {
        super.onViewportInitOnce(viewport);
        //
        this.webPane().addEventHandler(KeyEvent.KEY_PRESSED, this::handleWebViewShortcuts);

        addTool_Print();
        addTool_GotoHeadings();
        addTool_FindInPage();
    }

    protected void addTool_Print() {
        Button button = MaterialIcon.PRINT.flatButton();
        button.setText("打印");
        button.setTooltip(new Tooltip("添加到系统默认打印机"));
        button.setOnAction(event -> {
            Printer printer = Printer.getDefaultPrinter();
            if (null == printer) {
                app.toastError("打印机不可用");
                return;
            }

            PrinterJob job = PrinterJob.createPrinterJob(printer);
            PageLayout pageLayout = printer.createPageLayout(Paper.A4, PageOrientation.PORTRAIT,
                    .4, .4, .4, .4);
            job.getJobSettings().setPageLayout(pageLayout);
            webPane().webEngine().print(job);
            job.endJob();
            app.toast("已添加到系统打印队列，请检查打印结果！");
        });
        //
        webPane().getTopAsBar().addLeft(button);
    }

    private Button gotoHeadings;
    private LookupLayer<String> gotoHeadingsLayer;

    protected void addTool_GotoHeadings() {
        gotoHeadings = MaterialIcon.NEAR_ME.flatButton();
        gotoHeadings.setText("转到");
        gotoHeadings.setTooltip(new Tooltip("转到 (Ctrl+T)"));
        gotoHeadings.setOnAction(event -> {
            if (null == gotoHeadingsLayer) {
                gotoHeadingsLayer = new LookupLayer<>(getViewport()) {
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
                                            String dataInAscii = AppContext.ascii(text);
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
                        openedItem.attr("position.selector", "#".concat(arr[0]));

                        hide();
                        position(openedItem);
                    }

                    @Override
                    public void hide() {
                        super.hide();
                        webPane().webView().requestFocus();
                    }
                };
            }
            gotoHeadingsLayer.show(null);
        });
        //
        webPane().getTopAsBar().addLeft(gotoHeadings);
    }

    protected void addTool_FindInPage() {
        webFinder = new WebPane.WebMarksFinder(this.webPane(), getViewport());
        webFinder.setAsciiConvertor(AppContext::ascii);
        webPane().getTopAsBar().addRight(webFinder);
    }

    public void onViewportClosing(Event event, boolean selected) {
        super.onViewportClosing(event, selected);
        //
        app.eventBus.fireEvent(new ItemEvent(ItemEvent.VISITED, item));
    }

    @Override
    protected void saveUserExperienceData() {
        if (null == this.webPane()) return;
        try {
            final double scrollTopPercentage = webPane().getScrollTopPercentage();
            UserPrefs.recents.setProperty(item.getPath().concat(".percent"), scrollTopPercentage);

            final String selector = webPane().executeScript("getScrollTop1Selector()");
//            System.out.println(selector + " SET selector for " + path.get());
            UserPrefs.recents.setProperty(item.getPath().concat(".selector"), selector);
        } catch (Exception ignore) {
        }
    }

    @Override
    protected void onWebEngineLoading() {
        navigate(null);
    }

    private Item openedItem, navigatePos;

    public void navigate(Item item) {
        final Item pos = null != item ? item : removeAttr(Item.class);
        if (null == item)
            item = this.item;

        if (null != openedItem && Objects.equals(item.getPath(), openedItem.getPath())) {
            openedItem = item;
            navigatePos = pos;
            position(pos);
            return;
        }

        if (null == progressLayerHandler) progressLayerHandler = ProgressLayer.showIndicator(getViewport());
        openedItem = item;
        navigatePos = null != navigatePos ? navigatePos : item;
        final Object html = prepareHtmlContent();
        if (html instanceof URI uri) {
            logger.warn("LOAD html: ".concat(uri.toString()));
            FxHelper.runLater(() -> webPane().webEngine().load(uri.toString()));
        } else if (html instanceof String text) {
            logger.warn("LOAD html, len=" + text.length());
            FxHelper.runLater(() -> webPane().webEngine().loadContent(text));
        }
    }

    protected Object prepareHtmlContent() {
        return Path.of(item.getPath()).toUri();
    }

    protected void onWebEngineLoadSucceeded() {
        // set an interface object named 'javaApp' in the web engine's page
        final JSObject window = webPane().executeScript("window");
        window.setMember("javaApp", javaApp);

        webPane().setContextMenuBuilder(this::handleWebViewContextMenu);
        //
        position(navigatePos);
        //
        webPane().widthProperty().removeListener(onWebViewBodyResize);
        webPane().widthProperty().addListener(onWebViewBodyResize);
        //
        super.onWebEngineLoadSucceeded();

        app.eventBus.fireEvent(new ItemEvent(ItemEvent.VISITED, this.item));
    }

    protected final void position(Attributes pos) {
        try {
            if (null != pos && pos.hasAttr("position.term")) {
                String posTerm = pos.removeAttr("position.term");
                String posText = pos.removeAttr("position.text"), longText = null;
                if (null != posText) {
                    List<String> posParts = new ArrayList<>(List.of(posText.split("。")));
                    posParts.sort(Comparator.comparingInt(String::length));
                    longText = posParts.get(posParts.size() - 1);
                }
                if (null != longText && webPane().findInPage(longText, true)) {
                    webFinder.mark(posTerm);
                } else if (null != longText && webPane().findInWindow(longText, true)) {
                    webFinder.mark(posTerm);
                } else {
                    webFinder.find(posTerm);
                }
            } else if (null != pos && pos.hasAttr("position.selector")) {
                webPane().executeScript(StringHelper.concat("setScrollTop1BySelectors(\"", pos.removeAttr("position.selector"), "\")"));
            } else if (null != pos && pos.hasAttr("anchor")) {
                webPane().executeScript("setScrollTop1BySelectors(\"".concat(pos.removeAttr("anchor")).concat("\")"));
            } else {
                final String selector = UserPrefs.recents.getString(item.getPath().concat(".selector"), null);
                final double percent = UserPrefs.recents.getDouble(item.getPath().concat(".percent"), 0);
                if (null != selector) {
//                    System.out.println(selector + " GET selector for " + path.get());
                    webPane().executeScript(StringHelper.concat("setScrollTop1BySelectors(\"", selector, "\")"));
                } else if (openedItem.hasAttr("anchor")) {
                    webPane().executeScript("setScrollTop1BySelectors(\"".concat(openedItem.attr("anchor")).concat("\")"));
                }
            }
        } catch (Throwable e) {
            e.printStackTrace();
        }
        webPane().webView().requestFocus();
    }

    protected void handleWebViewShortcuts(KeyEvent event) {
        if (!event.isConsumed() && event.isShortcutDown()) {
            // Ctrl + F
            if (event.getCode() == KeyCode.F) {
                // 如果有选中文字，则按查找选中文字处理
                String selText = webPane().executeScript("getValidSelectionText()");
                selText = null == selText ? null : selText.strip().replace('\n', ' ');
                webFinder.find(StringHelper.isBlank(selText) ? null : selText);
                event.consume();
                return;
            }
            // Ctrl + G, Ctrl + H
            if (event.getCode() == KeyCode.G || event.getCode() == KeyCode.H) {
                // 如果有选中文字，则按查找选中文字处理
                final String selText = webPane().executeScript("getValidSelectionText()");
                app.eventBus.fireEvent(event.getCode() == KeyCode.G
                        ? SearcherEvent.ofLookup(selText) // LOOKUP
                        : SearcherEvent.ofSearch(selText) // SEARCH
                );
                event.consume();
                return;
            }
            // Ctrl + T
            if (event.getCode() == KeyCode.T && null != gotoHeadings) {
                gotoHeadings.fire();
                event.consume();
                return;
            }
        }
    }

    protected ContextMenu handleWebViewContextMenu() {
        final String origText = webPane().executeScript("getValidSelectionText()");
        String trimText = null == origText ? null : origText.strip().replace('\n', ' ');
        final String availText = StringHelper.isBlank(trimText) ? null : trimText;
        //
        MenuItem copy = new MenuItem("复制");
        copy.setDisable(null == availText);
        copy.setOnAction(event -> Clipboard.getSystemClipboard().setContent(Map.of(DataFormat.PLAIN_TEXT, origText)));
        //
        MenuItem copyRef = new MenuItem("复制引用");
        copyRef.setDisable(null == availText);
        copyRef.setOnAction(event -> Clipboard.getSystemClipboard().setContent(Map.of(DataFormat.PLAIN_TEXT,
                "《".concat(item.getName()).concat("》\n\n").concat(origText))));
        //
        String textTip = null == availText ? "" : "：".concat(StringHelper.trimChars(availText, 8));

        MenuItem search = new MenuItem("全文检索".concat(textTip));
        search.setOnAction(event -> app.eventBus.fireEvent(SearcherEvent.ofSearch(availText)));

        MenuItem lookup = new MenuItem("快捷检索".concat(textTip));
        lookup.setOnAction(event -> app.eventBus.fireEvent(SearcherEvent.ofLookup(availText)));

        MenuItem finder = new MenuItem("页内查找".concat(textTip));
        finder.setOnAction(event -> webFinder.find(availText));
        //
        MenuItem dictionary = new MenuItem("查词典");
        dictionary.setDisable(true);
        //
        MenuItem bookmark = new MenuItem("添加书签");
        bookmark.setDisable(true);
//        bookmark.setOnAction(event -> this.handleEventToCreateBookdata(BookdataType.bookmark));

        MenuItem favorite = new MenuItem("添加收藏");
        favorite.setDisable(true);
//        favorite.setOnAction(event -> this.handleEventToCreateBookdata(BookdataType.favorite));


        //
        return new ContextMenu(copy, copyRef,
                new SeparatorMenuItem(),
                search, lookup, finder,
                new SeparatorMenuItem(),
                dictionary,
                new SeparatorMenuItem(),
                bookmark, favorite
        );
    }

    private void onWebViewBodyResize(Observable o) {
        webPane().executeScript("beforeOnResizeBody()");
    }

    ////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

    /**
     * for communication from the Javascript engine.
     */
    private final JavaApp javaApp = new JavaApp();

    public final class JavaApp {
        private JavaApp() {
        }

        public void updateFinderState(int index, int count) {
            webFinder.state(index, count);
        }
    }

    protected final static class WebIncl {
        private static final String[] includeNames = {
                "jquery.min.js", "jquery.ext.js",
                "jquery.isinviewport.js", "jquery.scrollto.js",
//                "jquery.highlight.js", "jquery.highlight.finder.js",
                "jquery.mark.js", "jquery.mark.finder.js",
                "popper.min.js", "tippy-bundle.umd.min.js",
                "rangy-core.js", "rangy-serializer.js",
                "app.css", "app.js"
        };
        private static final String[] includePaths = new String[includeNames.length];

        public static String[] getIncludePaths() {
            if (null != includePaths[0])
                return includePaths;

            synchronized (includePaths) {
                if (null != includePaths[0])
                    return includePaths;

                final Path dir = DesktopApp.appDir().resolve("template/web-incl");
                for (int i = 0; i < includeNames.length; i++) {
                    includePaths[i] = dir.resolve(includeNames[i]).toUri().toString();
                }
            }
            return includePaths;
        }
    }
}
