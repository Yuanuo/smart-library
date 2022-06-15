package org.appxi.smartlib.app.search;

import javafx.scene.control.ContentDisplay;
import javafx.scene.control.Label;
import javafx.scene.control.Labeled;
import javafx.scene.input.InputEvent;
import javafx.scene.input.KeyCode;
import javafx.scene.input.KeyCodeCombination;
import javafx.scene.input.KeyCombination;
import javafx.scene.input.KeyEvent;
import javafx.scene.layout.HBox;
import javafx.scene.layout.Priority;
import javafx.scene.layout.StackPane;
import org.appxi.event.EventHandler;
import org.appxi.javafx.app.search.SearcherEvent;
import org.appxi.javafx.control.LookupLayer;
import org.appxi.javafx.helper.FxHelper;
import org.appxi.javafx.visual.MaterialIcon;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.javafx.workbench.WorkbenchPart;
import org.appxi.javafx.workbench.WorkbenchPartController;
import org.appxi.prefs.UserPrefs;
import org.appxi.search.solr.Piece;
import org.appxi.smartlib.Item;
import org.appxi.smartlib.ItemEvent;
import org.appxi.smartlib.ItemProviders;
import org.appxi.smartlib.app.item.ItemEx;
import org.appxi.smartlib.dao.BeansContext;
import org.appxi.smartlib.dao.PiecesRepository;
import org.appxi.util.ext.LookupExpression;
import org.springframework.data.domain.Page;
import org.springframework.data.solr.core.query.SolrPageRequest;

import java.util.Collection;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Set;

public class LookupController extends WorkbenchPartController implements WorkbenchPart.SideTool {
    public LookupController(WorkbenchPane workbench) {
        super(workbench);

        this.id.set("LOOKUP");
        this.tooltip.set("快捷检索 (Ctrl+G)");
        this.graphic.set(MaterialIcon.NEAR_ME.graphic());
    }

    @Override
    public boolean sideToolAlignTop() {
        return true;
    }

    private long lastShiftKeyPressedTime;

    @Override
    public void postConstruct() {
        app.getPrimaryScene().getAccelerators().put(new KeyCodeCombination(KeyCode.G, KeyCombination.SHORTCUT_DOWN),
                () -> this.activeViewport(false));
        app.getPrimaryScene().addEventHandler(KeyEvent.KEY_PRESSED, evt -> {
            if (evt.getCode() == KeyCode.SHIFT) {
                final long currShiftKeyPressedTime = System.currentTimeMillis();
                if (currShiftKeyPressedTime - lastShiftKeyPressedTime <= 400) {
                    this.activeViewport(false);
                } else lastShiftKeyPressedTime = currShiftKeyPressedTime;
            }
        });
        app.eventBus.addEventHandler(SearcherEvent.LOOKUP,
                event -> this.lookup(null != event.text ? event.text.strip() : null));
    }

    @Override
    public void activeViewport(boolean firstTime) {
        lookup(null);
    }

    private LookupLayer<Piece> lookupLayer;
    private String lookupText;

    private void lookup(String text) {
        if (null == lookupLayer) {
            lookupLayer = new LookupLayerImpl(app.getPrimaryGlass());

            app.eventBus.addEventHandler(ItemEvent.VIEWING, event -> lookupLayer.refresh());
            //
            final EventHandler<ItemEvent> itemChangedEventHandler = event -> lookupLayer.reset();
            app.eventBus.addEventHandler(ItemEvent.UPDATED, itemChangedEventHandler);
            app.eventBus.addEventHandler(ItemEvent.DELETED, itemChangedEventHandler);
            app.eventBus.addEventHandler(ItemEvent.RENAMED, itemChangedEventHandler);
            app.eventBus.addEventHandler(ItemEvent.MOVED, itemChangedEventHandler);
        }
        lookupLayer.show(text != null ? text : lookupText);
    }

    class LookupLayerImpl extends LookupLayer<Piece> {
        public LookupLayerImpl(StackPane glassPane) {
            super(glassPane);
        }

        @Override
        protected int getPaddingSizeOfParent() {
            return 200;
        }

        @Override
        protected String getHeaderText() {
            return "快捷检索";
        }

        @Override
        protected String getUsagesText() {
            return """
                    >> 空格分隔任意字/词/短语匹配；
                    >> 快捷键：双击Shift 或 Ctrl+G 开启；ESC 或 点击透明区 退出此界面；上/下方向键选择列表项；回车键打开；
                    """;
        }

        private Set<String> usedKeywords;

        @Override
        protected void updateItemLabel(Labeled labeled, Piece item) {
            // TODO show more info
            Label title = new Label(item.title);
            title.getStyleClass().addAll("primary", "plaintext");

            Label detail = new Label(item.path, MaterialIcon.LOCATION_ON.graphic());
            detail.getStyleClass().add("secondary");

            HBox.setHgrow(title, Priority.ALWAYS);
            HBox pane = new HBox(5, title, detail);

            labeled.setText(title.getText());
            labeled.setGraphic(pane);
            labeled.setContentDisplay(ContentDisplay.GRAPHIC_ONLY);

            labeled.getStyleClass().remove("visited");
            if (null != item.path && UserPrefs.recents.containsProperty(item.path)) {
                labeled.getStyleClass().add("visited");
            }
            FxHelper.highlight(title, usedKeywords);
        }

        @Override
        protected Collection<Piece> lookupByKeywords(String lookupText, int resultLimit) {
            LookupController.this.lookupText = lookupText;
            usedKeywords = new LinkedHashSet<>();
            LookupExpression.of(lookupText).ifPresent(
                    expr -> usedKeywords.addAll(expr.keywords().stream().map(v -> v.keyword()).toList()));
            //
            final PiecesRepository repository = BeansContext.getBean(PiecesRepository.class);
            if (null == repository) return List.of();
            Page<Piece> result = repository.lookup(
                    null,
                    List.of("article", "topic", "label", "location"),
                    lookupText,
                    null,
                    new SolrPageRequest(0, resultLimit + 1));
            return result.getContent();
        }

        @Override
        protected void lookupByCommands(String searchTerm, Collection<Piece> result) {
        }

        @Override
        protected void handleEnterOrDoubleClickActionOnSearchResultList(InputEvent event, Piece piece) {
            if (null == piece)
                return;

            final Item item = new ItemEx(piece.title, piece.path, ItemProviders.find(piece.provider));
            final String anchor = piece.field("anchor_s");
            if (null != anchor)
                item.attr("position.selector", "#".concat(anchor));

            hide();
            app.eventBus.fireEvent(new ItemEvent(ItemEvent.VIEWING, item));
        }
    }
}
