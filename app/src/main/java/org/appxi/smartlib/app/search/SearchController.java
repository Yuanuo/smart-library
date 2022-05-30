package org.appxi.smartlib.app.search;

import javafx.geometry.Pos;
import javafx.scene.input.KeyCode;
import javafx.scene.input.KeyCodeCombination;
import javafx.scene.input.KeyCombination;
import org.appxi.javafx.control.OpaqueLayer;
import org.appxi.javafx.helper.FxHelper;
import org.appxi.javafx.visual.MaterialIcon;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.javafx.workbench.views.WorkbenchSideToolController;
import org.appxi.smartlib.Item;
import org.appxi.smartlib.ItemEvent;
import org.appxi.smartlib.app.event.SearchedEvent;
import org.appxi.smartlib.app.event.SearcherEvent;
import org.appxi.smartlib.dao.ItemsDao;
import org.appxi.util.DigestHelper;

import java.util.Objects;

public class SearchController extends WorkbenchSideToolController {

    public SearchController(WorkbenchPane workbench) {
        super("SEARCH", workbench);
        this.setTitles("搜索", "全文检索 (Ctrl+H)");
        this.attr(Pos.class, Pos.CENTER_LEFT);
        this.graphic.set(MaterialIcon.SEARCH.graphic());
    }

    @Override
    public void initialize() {
        // 响应快捷键 Ctrl+H 事件，以打开搜索视图
        app.getPrimaryScene().getAccelerators().put(new KeyCodeCombination(KeyCode.H, KeyCombination.SHORTCUT_DOWN),
                () -> openSearcherWithText(null, null));
        // 响应SEARCH Event事件，以打开搜索视图
        app.eventBus.addEventHandler(SearcherEvent.SEARCH, event -> openSearcherWithText(event.text, event.scope));
        // 响应搜索结果打开事件
        app.eventBus.addEventHandler(SearchedEvent.OPEN, event -> {
            if (null == event.piece)
                return;

            final Item item = ItemsDao.items().resolve(event.piece.path);

            String anchor = event.piece.field("anchor_s");
            if (null != anchor)
                item.attr("position.selector", anchor);
            if (null != event.highlightTerm)
                item.attr("position.term", event.highlightTerm.replaceAll("…|^\"|\"$", ""));
            if (null != event.highlightSnippet)
                item.attr("position.text", event.highlightSnippet
                        .replace("§§hl#end§§", "").replace("…", ""));

            app.eventBus.fireEvent(new ItemEvent(ItemEvent.VIEWING, item));
        });
    }

    @Override
    public void onViewportShowing(boolean firstTime) {
        openSearcherWithText(null, null);
    }

    private void openSearcherWithText(String text, Item scope) {
        // 有从外部打开的全文搜索，此时需要隐藏透明层
        OpaqueLayer.hideOpaqueLayer(app.getPrimaryGlass());

        // 优先查找可用的搜索视图，以避免打开太多未使用的搜索视图
        SearcherController searcher = workbench.mainViews.getTabs().stream()
                .map(tab -> (tab.getUserData() instanceof SearcherController view && view.isNeverSearched()) ? view : null)
                .filter(Objects::nonNull)
                .findFirst()
                .orElseGet(() -> new SearcherController("SEARCHER-".concat(DigestHelper.uid()), workbench));
        FxHelper.runLater(() -> {
            if (!workbench.existsMainView(searcher.id.get())) {
                workbench.addWorkbenchViewAsMainView(searcher, false);
                searcher.initialize();
            }
            workbench.selectMainView(searcher.id.get());
            searcher.setSearchScope(scope);
            searcher.search(text);
        });
    }

}