package org.appxi.smartlib.app.search;

import javafx.beans.InvalidationListener;
import javafx.beans.property.SimpleBooleanProperty;
import javafx.collections.ListChangeListener;
import javafx.geometry.Pos;
import javafx.scene.Node;
import javafx.scene.control.Button;
import javafx.scene.control.ContextMenu;
import javafx.scene.control.Hyperlink;
import javafx.scene.control.Label;
import javafx.scene.control.ListCell;
import javafx.scene.control.ListView;
import javafx.scene.control.MenuItem;
import javafx.scene.control.Pagination;
import javafx.scene.control.ScrollPane;
import javafx.scene.control.SplitPane;
import javafx.scene.control.Tab;
import javafx.scene.control.TabPane;
import javafx.scene.control.TextField;
import javafx.scene.control.Tooltip;
import javafx.scene.control.cell.CheckBoxListCell;
import javafx.scene.input.MouseButton;
import javafx.scene.input.TransferMode;
import javafx.scene.layout.BorderPane;
import javafx.scene.layout.HBox;
import javafx.scene.layout.Pane;
import javafx.scene.layout.Priority;
import javafx.scene.layout.StackPane;
import javafx.scene.layout.VBox;
import javafx.scene.text.Text;
import javafx.scene.text.TextAlignment;
import javafx.scene.text.TextFlow;
import javafx.util.Callback;
import org.appxi.holder.BoolHolder;
import org.appxi.javafx.app.search.SearchedEvent;
import org.appxi.javafx.control.ListViewEx;
import org.appxi.javafx.control.ProgressLayer;
import org.appxi.javafx.helper.FxHelper;
import org.appxi.javafx.visual.MaterialIcon;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.javafx.workbench.WorkbenchPartController;
import org.appxi.search.solr.Piece;
import org.appxi.smartcn.pinyin.PinyinHelper;
import org.appxi.smartlib.dao.BeansContext;
import org.appxi.smartlib.dao.PiecesRepository;
import org.appxi.util.StringHelper;
import org.appxi.util.ext.Period;
import org.appxi.util.ext.RawVal;
import org.springframework.data.solr.core.query.SolrPageRequest;
import org.springframework.data.solr.core.query.result.FacetAndHighlightPage;
import org.springframework.data.solr.core.query.result.HighlightEntry;

import java.util.ArrayList;
import java.util.Comparator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.Optional;
import java.util.function.Consumer;
import java.util.stream.Collectors;

import static org.appxi.smartlib.app.item.ItemEx.DND_ITEM;

class SearcherController extends WorkbenchPartController.MainView {
    static final int PAGE_SIZE = 10;

    public SearcherController(String viewId, WorkbenchPane workbench) {
        super(workbench);

        this.id.set(viewId);
        this.setTitles(null);
    }

    protected void setTitles(String appendText) {
        String title = "??????";
        if (null != appendText)
            title = title + "???" + (appendText.isBlank() ? "*" : StringHelper.trimChars(appendText, 16));
        this.title.set(title);
        this.tooltip.set(title);
        this.appTitle.set(title);
    }

    @Override
    public void postConstruct() {
    }

    private FirstView firstView;
    private TextField input;
    private TabPane filterTabs;
    private BorderPane resultPane;
    private Label resultInfo;

    private String inputQuery, finalQuery;

    @Override
    protected void createViewport(StackPane viewport) {
        super.createViewport(viewport);
        //
        firstView = new FirstView(this::search);
        //
        final SplitPane splitPane = new SplitPane();
        HBox.setHgrow(splitPane, Priority.ALWAYS);
        splitPane.getStyleClass().add("search-view");

        input = new TextField();
        HBox.setHgrow(input, Priority.ALWAYS);

        final Button submit = new Button("???");
        submit.setOnAction(event -> this.search(""));
        input.setOnAction(event -> submit.fire());

        filterTabs = new TabPane(
                new FacetsTab("/library/", "??????"),
                new FacetsTab("/period/", "??????"),
                new FacetsTab("/catalog/", "???"),
                new FacetsTab("/author/", "?????????"),
                new ScopesTab("??????"),
                new UsagesTab());
        filterTabs.getStyleClass().addAll("filters", "compact");
        filterTabs.setTabClosingPolicy(TabPane.TabClosingPolicy.UNAVAILABLE);
        VBox.setVgrow(filterTabs, Priority.ALWAYS);

        resultPane = new BorderPane();
        resultPane.setBottom(resultInfo = new Label());
        BorderPane.setAlignment(resultInfo, Pos.CENTER);
        resultInfo.setStyle(resultInfo.getStyle().concat("-fx-padding:.5em;"));
        resultInfo.getStyleClass().add("result-info");

        final HBox inputBox = new HBox(input, submit);
        inputBox.setStyle("-fx-padding: 1em 0;");

        final VBox vBox = new VBox();
        vBox.setStyle("-fx-padding:.5em;-fx-spacing:.5em;");
        vBox.getChildren().addAll(inputBox, filterTabs);
        SplitPane.setResizableWithParent(vBox, false);
        splitPane.getItems().setAll(vBox, resultPane);
        splitPane.setDividerPositions(.3);

        viewport.getChildren().addAll(splitPane, firstView);
    }

    @Override
    public void activeViewport(boolean firstTime) {
        if (null != firstView) {
            firstView.requestFocus();
        }
    }

    @Override
    public void inactiveViewport(boolean closing) {
    }

    boolean isNeverSearched() {
        // inputView????????????????????????????????????????????????
        return null != firstView;
    }

    void setSearchScopes(RawVal<String>... scopes) {
        filterTabs.getTabs().stream()
                .filter(t -> t instanceof ScopesTab)
                .findFirst()
                .ifPresent(t -> ((ScopesTab) t).listView.getItems().setAll(scopes));
    }

    void search(String text) {
        if (null == text)
            return;
        if (null != firstView) {
            FxHelper.runLater(() -> {
                getViewport().getChildren().remove(firstView);
                firstView = null;
            });
            input.setText(text);
        }
        // ??????????????????????????????????????????????????????????????????????????????????????????????????????????????????
        String inputText = input.getText().strip().replace(':', ' ');
        //
        inputQuery = inputText;
        // ???????????????????????????
        if (!inputText.isEmpty() && (inputText.charAt(0) == '!' || inputText.charAt(0) == '???')) {
            // ?????????????????????????????????????????????????????????????????????????????????????????????????????????????????????????????????
            inputText = inputText.substring(1).strip();
        } else {
            inputText = inputText.strip();
        }
        // ??????????????????????????????????????????
        if (inputText.length() > 20)
            inputText = inputText.substring(0, 20);
        //
        searching(inputText);
    }

    private Runnable progressLayerRemover;
    private FacetAndHighlightPage<Piece> facetAndHighlightPage;

    private void searching(String inputText) {
        setTitles(inputQuery.length() > 20 ? inputQuery.substring(0, 20) : inputQuery);
        // ?????????????????????UI????????????
        progressLayerRemover = ProgressLayer.show(getViewport(), progressLayer -> searchingImpl(inputText));
    }

    private void searchingImpl(String inputText) {
        final PiecesRepository repository = BeansContext.getBean(PiecesRepository.class);
        if (null == repository) return;
        // ??????????????????
        final BoolHolder facet = new BoolHolder(true);
        final List<String> filterTypes = List.of("article", "topic");

        final List<String> filterCategories = new ArrayList<>();
        // ?????????????????????????????????????????????????????????????????????????????????????????????facet?????????????????????????????????????????????
        if (Objects.equals(inputText, finalQuery) && null != facetAndHighlightPage && facetAndHighlightPage.getTotalElements() > 0) {
            facet.value = false;
            filterTabs.getTabs().stream()
                    .filter(t -> t instanceof FacetsTab)
                    .map(t -> (FacetsTab) t)
                    .forEach(t -> filterCategories.addAll(t.getSelectedValues()));
        }

        final List<String> filterScopes = new ArrayList<>();
        filterTabs.getTabs().stream()
                .filter(t -> t instanceof ScopesTab)
                .map(t -> (ScopesTab) t)
                .forEach(t -> filterScopes.addAll(t.getSelectedValues()));

        finalQuery = inputText;
        // ??????
        facetAndHighlightPage = repository.search(filterScopes, filterTypes,
                finalQuery, filterCategories, facet.value, new SolrPageRequest(0, PAGE_SIZE));
        // debug
        if (null == facetAndHighlightPage) {
            FxHelper.runLater(() -> Optional.ofNullable(progressLayerRemover).ifPresent(Runnable::run));
            return;
        }

        // ??????????????????
        // ??????Facet????????????
        final Map<String, List<FacetItem>> facetListMap = new LinkedHashMap<>(4);
        if (facet.value) {
            filterTabs.getTabs().stream()
                    .filter(t -> t instanceof FacetsTab)
                    .map(t -> (FacetsTab) t)
                    .forEach(t -> facetListMap.put(t.getId(), new ArrayList<>()));

            facetAndHighlightPage.getFacetResultPages().forEach(p -> p.getContent().forEach(entry -> {
                String value = entry.getValue();
                long count = entry.getValueCount();

                for (String k : facetListMap.keySet()) {
                    if (!value.contains(k))
                        continue;

                    String label = value.split(k, 2)[1];
                    String order = PinyinHelper.convert(label, "-", false);
                    facetListMap.get(k).add(new FacetItem(value, label, count, order));
                    break;
                }
            }));
            facetListMap.forEach((id, list) -> {
                if ("/period/".equals(id)) {
                    // ?????????????????????
                    list.forEach(f -> Optional.ofNullable(Period.valueBy(f.label))
                            .ifPresentOrElse(p -> f.update(p.toString(), p.start), () -> f.order = 99999));
                    list.sort(Comparator.comparingInt(v -> (int) v.order));
                } else {
                    // ?????????????????????
                    list.sort(Comparator.comparing(v -> String.valueOf(v.order)));
                }
            });
        }
        FxHelper.runLater(() -> {
            if (facet.value) {
                // ??????Facet????????????
                filterTabs.getTabs().stream()
                        .filter(t -> t instanceof FacetsTab)
                        .map(t -> (FacetsTab) t)
                        .forEach(t -> t.listView.getItems().setAll(facetListMap.get(t.getId())));
            }

            // ??????????????????
            if (facetAndHighlightPage.getTotalElements() > 0) {
                Pagination pagination = new Pagination(facetAndHighlightPage.getTotalPages(), 0);
                pagination.setPageFactory(pageIdx -> createPage(pageIdx, repository,
                        filterTypes, filterCategories, filterScopes));
                resultPane.setCenter(pagination);
            } else {
                resultPane.setCenter(new Label("??????????????????????????????????????????????????????????????????"));
                resultInfo.setText("");
            }
            if (null != progressLayerRemover) progressLayerRemover.run();
        });
    }

    private Node createPage(int pageIdx,
                            PiecesRepository repository,
                            List<String> filterTypes,
                            List<String> filterCategories,
                            List<String> filterScopes) {
        if (null == repository) return new Label();
        final FacetAndHighlightPage<Piece> highlightPage;
        if (pageIdx == 0) {
            // use exists data
            highlightPage = facetAndHighlightPage;
        } else {
            // query for next page
            highlightPage = repository.search(filterScopes, filterTypes,
                    finalQuery, filterCategories, false, new SolrPageRequest(pageIdx, PAGE_SIZE));
        }
        if (null == highlightPage)
            return new Label();// avoid NPE

        final VBox listView = new VBox();
        highlightPage.getContent().forEach(v -> listView.getChildren().add(new PieceView(v, highlightPage.getHighlights(v))));

        if (highlightPage.getTotalElements() > 0) {
            resultInfo.setText("??? %d ?????????   ???????????????%d - %d   ?????????%d / %d".formatted(
                    highlightPage.getTotalElements(),
                    pageIdx * PAGE_SIZE + 1, Math.min((long) (pageIdx + 1) * PAGE_SIZE, highlightPage.getTotalElements()),
                    pageIdx + 1,
                    highlightPage.getTotalPages()
            ));
        } else {
            resultInfo.setText("");
        }

        final ScrollPane scrollPane = new ScrollPane(listView);
        scrollPane.setFitToWidth(true);
        scrollPane.setStyle("-fx-padding: 0;-fx-background-insets: 0;");
        return scrollPane;
    }

    static class FacetItem {
        final SimpleBooleanProperty stateProperty = new SimpleBooleanProperty();
        final String value;
        String label;
        final long count;
        Object order;

        FacetItem(String value, String label, long count, Object order) {
            this.value = value;
            this.label = label;
            this.count = count;
            this.order = order;
        }

        void update(String label, Object order) {
            this.label = label;
            this.order = order;
        }

        @Override
        public String toString() {
            return "%s???%d???".formatted(label, count);
        }
    }

    class FacetsTab extends Tab implements Callback<ListView<FacetItem>, ListCell<FacetItem>> {
        final ListView<FacetItem> listView;
        final InvalidationListener facetCellStateListener;

        FacetsTab(String id, final String label) {
            super(label);
            setId(id);

            facetCellStateListener = o -> {
                // ??????Tab????????????
                setText(getSelectedItems().isEmpty() ? label : label + "*");
                searching(finalQuery);
            };

            listView = new ListViewEx<>((event, item) -> {
                // ???????????????????????????
                filterTabs.getTabs().stream()
                        .filter(t -> t instanceof FacetsTab)
                        .map(t -> (FacetsTab) t)
                        .forEach(t -> {
                            List<FacetItem> selectedList = t.getSelectedItems();
                            // remove listener
                            selectedList.forEach(v -> v.stateProperty.removeListener(t.facetCellStateListener));
                            // clean selected state
                            selectedList.forEach(v -> v.stateProperty.set(false));
                            // rebind listener
                            selectedList.forEach(v -> v.stateProperty.addListener(t.facetCellStateListener));
                        });
                // ?????????????????????????????????
                item.stateProperty.set(true);
            });
            listView.setCellFactory(this);

            VBox.setVgrow(listView, Priority.ALWAYS);
            setContent(new VBox(listView));
        }

        List<FacetItem> getSelectedItems() {
            return listView.getItems().stream().filter(v -> v.stateProperty.get()).toList();
        }

        List<String> getSelectedValues() {
            return listView.getItems().stream().filter(v -> v.stateProperty.get()).map(v -> v.value).toList();
        }

        @Override
        public ListCell<FacetItem> call(ListView<FacetItem> param) {
            return new CheckBoxListCell<>(val -> {
                val.stateProperty.removeListener(facetCellStateListener);
                val.stateProperty.addListener(facetCellStateListener);
                return val.stateProperty;
            });
        }
    }

    class ScopesTab extends Tab {
        final ListView<RawVal<String>> listView;

        ScopesTab(final String label) {
            super(label);

            final Label usageTip = new Label("""
                    ??????????????????????????????????????????????????????
                    ?????????????????????????????????????????????????????????????????????????????????
                    ??????????????????????????????????????????????????????????????????
                    """);
            usageTip.setWrapText(true);
            usageTip.setStyle("-fx-padding:.5em;");
            usageTip.setLineSpacing(5);
            usageTip.getStyleClass().add("bob-line");

            listView = new ListView<>();
            // ??????????????????????????????????????????
            listView.getItems().addListener((ListChangeListener<? super RawVal<String>>) c -> {
                // ??????Tab????????????
                super.setText(listView.getItems().isEmpty() ? label : label + "*");
                // ?????????????????????????????????????????????????????????
                if (!isNeverSearched()) {
                    final String oldStr = finalQuery;
                    finalQuery = null; // reset
                    searching(oldStr);
                }
            });
            // ??????????????????????????????
            final MenuItem removeSel = new MenuItem("????????????");
            removeSel.setOnAction(e -> listView.getItems().removeAll(listView.getSelectionModel().getSelectedItems()));
            final MenuItem removeAll = new MenuItem("????????????");
            removeAll.setOnAction(e -> listView.getItems().clear());
            listView.setContextMenu(new ContextMenu(removeSel, removeAll));

            VBox.setVgrow(listView, Priority.ALWAYS);
            final VBox vBox = new VBox(usageTip, listView);
            setContent(vBox);
            // ?????????????????????????????????????????????????????????????????????????????????
            vBox.setOnDragOver(e -> e.acceptTransferModes(e.getDragboard().hasContent(DND_ITEM) ? TransferMode.ANY : TransferMode.NONE));
            vBox.setOnDragDropped(e -> {
                if (e.getDragboard().hasContent(DND_ITEM)) {
                    final RawVal<String> data = RawVal.kv(e.getDragboard().getString(), (String) e.getDragboard().getContent(DND_ITEM));
                    // remove old one
                    // ?????????????????????????????????????????????????????????????????????????????????????????????????????????
                    listView.getItems().removeIf(rv -> rv.value().startsWith(data.value()) || data.value().startsWith(rv.value()));
                    // add new one
                    listView.getItems().add(data);
                    e.setDropCompleted(true);
                }
            });
        }

        List<String> getSelectedValues() {
            return listView.getItems().stream().map(RawVal::value).toList();
        }
    }

    static class UsagesTab extends Tab {
        UsagesTab() {
            super("???");

            final Label usageTip = new Label("""
                    1???????????????????????????????????????????????????????????????????????????????????????
                    2?????????????????????????????????????????????
                    3??????????????????????????????????????????????????????????????????
                    4?????????????????????????????????????????????????????????????????????????????????????????????????????????
                    5??????????????????????????????????????????????????????20???
                    6???????????????Ctrl + H?????????????????????????????????????????????????????????
                    7????????????????????????????????????????????????????????????????????????????????????????????????
                    """);
            usageTip.setWrapText(true);
            usageTip.setStyle("-fx-padding:.5em;");
            usageTip.setLineSpacing(5);

            setContent(usageTip);
        }
    }

    class PieceView extends VBox {
        PieceView(Piece item, List<HighlightEntry.Highlight> highlights) {
            final Hyperlink nameLabel = new Hyperlink(item.title);
            nameLabel.getStyleClass().add("name");
            nameLabel.setStyle(nameLabel.getStyle().concat("-fx-font-size: 120%;"));
            nameLabel.setWrapText(true);
            nameLabel.setOnAction(e -> app.eventBus.fireEvent(new SearchedEvent(item, inputQuery, null)));

            final Label locationLabel = new Label(item.path, MaterialIcon.LOCATION_ON.graphic());
            locationLabel.getStyleClass().add("location");
            locationLabel.setStyle(locationLabel.getStyle().concat("-fx-opacity:.75;"));
            locationLabel.setWrapText(true);

            final Label authorsLabel = new Label(item.field("authors_s"), MaterialIcon.PEOPLE.graphic());
            authorsLabel.getStyleClass().add("authors");
            authorsLabel.setStyle(authorsLabel.getStyle().concat("-fx-opacity:.75;"));
            authorsLabel.setWrapText(true);

            final TextFlow textFlow = new TextFlow();
            textFlow.getStyleClass().add("text-flow");
            //
            this.getStyleClass().addAll("list-cell", "bob-line", "result-item");
            this.setStyle("-fx-spacing:.85em;-fx-padding:.85em .5em;-fx-font-size: 110%;-fx-opacity:.9;");
            this.getChildren().setAll(nameLabel, locationLabel, authorsLabel, textFlow);
            this.setOnMouseReleased(e -> {
                if (e.getButton() == MouseButton.PRIMARY && e.getClickCount() > 1) {
                    nameLabel.fire();
                } else if (e.getButton() == MouseButton.SECONDARY) {
                    FxHelper.copyText("?????????" + nameLabel.getText() +
                                      "\n?????????" + locationLabel.getText() +
                                      "\n????????????" + authorsLabel.getText() +
                                      "\n?????????\n" + textFlow.getChildren().stream().filter(n -> n instanceof Text)
                                              .map(n -> ((Text) n).getText())
                                              .collect(Collectors.joining()));
                    app.toast("????????????????????????");
                }
            });
            //
            List<Node> texts = new ArrayList<>();
            if (!highlights.isEmpty()) {
                highlights.forEach(highlight -> {
                    highlight.getSnipplets().forEach(str -> {
                        String[] strings = ("???" + str + "???").split("????hl#pre????");
                        for (String string : strings) {
                            String[] tmpArr = string.split("????hl#end????", 2);
                            if (tmpArr.length == 1) {
                                final Text text1 = new Text(tmpArr[0]);
                                text1.getStyleClass().add("plaintext");
                                texts.add(text1);
                            } else {
                                final Text hlText = new Text(tmpArr[0]);
                                hlText.getStyleClass().add("highlight");
                                hlText.setStyle(hlText.getStyle().concat("-fx-cursor:hand;"));
                                hlText.setOnMouseReleased(event -> {
                                    if (event.getButton() == MouseButton.PRIMARY) {
                                        app.eventBus.fireEvent(new SearchedEvent(item, tmpArr[0], string));
                                    }
                                });
                                texts.add(hlText);
                                final Text text1 = new Text(tmpArr[1]);
                                text1.getStyleClass().add("plaintext");
                                texts.add(text1);
                            }
                        }
                        texts.add(new Text("\n"));
                    });
                });
            } else {
                String text = item.text("text_txt_aio_sub");
                final Text text1 = new Text(null == text ? "" : StringHelper.trimChars(text, 200));
                text1.getStyleClass().add("plaintext");
                texts.add(text1);
            }
            textFlow.getChildren().setAll(texts.toArray(new Node[0]));
        }
    }

    class FirstView extends BorderPane {
        final TextField _input;

        FirstView(Consumer<String> enterAction) {
            super();
            this.getStyleClass().add("input-view");
            this.setStyle(";-fx-background-color:-fx-background;");

            _input = new TextField();
            _input.setPromptText("?????????????????????");
            _input.setTooltip(new Tooltip("??????????????????????????????????????????"));
            _input.setAlignment(Pos.CENTER);
            _input.setPrefColumnCount(50);

            final Button search = new Button("??????");
            search.setTooltip(new Tooltip("?????????????????????"));
            search.setOnAction(event -> enterAction.accept(_input.getText()));
            _input.setOnAction(event -> search.fire());

            final Button searchAll = new Button("??????");
            searchAll.setTooltip(new Tooltip("??????????????????"));
            searchAll.setOnAction(event -> enterAction.accept(""));

            final Button searchScope = new Button("???????????????");
            searchScope.setTooltip(new Tooltip("?????????????????????"));
            searchScope.setOnAction(event -> {
                input.setText(_input.getText());
                if (getParent() instanceof Pane pane) {
                    pane.getChildren().remove(this);
                }
                filterTabs.getTabs().stream()
                        .filter(t -> t instanceof ScopesTab)
                        .findFirst()
                        .ifPresent(t -> filterTabs.getSelectionModel().select(t));
            });

            final HBox actionsBox = new HBox(search, searchAll, searchScope);
            actionsBox.setAlignment(Pos.CENTER);
            actionsBox.setSpacing(20);

            final Label usageTip = new Label("""
                    ?????????

                    ????????????????????????????????????????????????????????????????????????????????????
                    ?????????????????????????????????
                    ???????????????????????????????????????
                    ??????????????????????????????????????????????????????????????????????????????
                    ???????????????????????????????????????????????????????????????????????????????????????????????????
                    ???????????????????????????????????????????????????20?????????????????????
                    ????????????Ctrl + H?????????????????????????????????????????????????????????
                    """);
            usageTip.setTextAlignment(TextAlignment.CENTER);
            usageTip.setWrapText(true);
            usageTip.setStyle("-fx-padding:3em 1em;");
            usageTip.setLineSpacing(5);

            final VBox vBox = new VBox(_input, actionsBox, usageTip);
            vBox.setAlignment(Pos.CENTER);
            vBox.setStyle("-fx-padding:10em 2em 0; -fx-spacing:2em;");
            final ScrollPane scrollPane = new ScrollPane(vBox);
            scrollPane.setFitToWidth(true);
            this.setCenter(scrollPane);
        }

        @Override
        public void requestFocus() {
            FxHelper.runThread(50, _input::requestFocus);
        }
    }
}
