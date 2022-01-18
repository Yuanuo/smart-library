package org.appxi.smartlib.item.article;

import javafx.geometry.Pos;
import javafx.scene.control.ButtonType;
import javafx.scene.control.ChoiceBox;
import javafx.scene.control.Dialog;
import javafx.scene.control.DialogPane;
import javafx.scene.control.Label;
import javafx.scene.control.TextArea;
import javafx.scene.control.TextField;
import javafx.scene.layout.HBox;
import javafx.scene.layout.Priority;
import javafx.scene.layout.VBox;
import org.appxi.holder.BoolHolder;
import org.appxi.smartlib.App;
import org.appxi.smartlib.search.Searchable;
import org.appxi.util.NumberHelper;
import org.appxi.util.StringHelper;

import java.util.stream.Collectors;
import java.util.stream.IntStream;

class ArticleMetadata extends DialogPane {
    final ArticleDocument document;
    final VBox form;
    TextField library, album;
    TextArea catalog;
    TextArea periods;
    TextArea authors;
    ChoiceBox<Integer> priority;
    ChoiceBox<Searchable> searchable;

    public ArticleMetadata(ArticleDocument document) {
        super();
        this.document = document;

        this.setMinSize(1280, 576);
        this.setHeaderText(document.item.typedPath());
        this.getButtonTypes().add(ButtonType.OK);

        this.form = new VBox(10);
        this.form.getStyleClass().add("form-vbox");
        //
        this.edit_library();
        this.edit_catalog();
        this.edit_album();
        this.edit_periods();
        this.edit_authors();
        this.edit_priority();
        this.edit_indexable();
        //
        this.setContent(form);
    }

    private void edit_library() {
        final Label label = new Label("库");
        label.getStyleClass().add("field-name");

        library = new TextField("unknown");
        HBox.setHgrow(library, Priority.ALWAYS);
        //
        final String val = document.getMetadata("library", null);
        if (StringHelper.isNotBlank(val)) {
            library.setText(val);
        }

        //
        final Label tipInfo = new Label("表示顶级类库，支持多级路径。如“大正藏”或“藏经/大正藏”");
        tipInfo.getStyleClass().add("field-info");
        //
        final HBox hBox = new HBox(label, library, tipInfo);
        hBox.setAlignment(Pos.CENTER_LEFT);
        this.form.getChildren().addAll(hBox);
    }

    private void edit_album() {
        final Label label = new Label("专辑");
        label.getStyleClass().add("field-name");

        album = new TextField();
        HBox.setHgrow(album, Priority.ALWAYS);
        //
        final String val = document.getMetadata("album", null);
        if (StringHelper.isNotBlank(val)) {
            album.setText(val);
        }

        //
        final Label tipInfo = new Label("所属专辑或系列。如“大般若经”或“心经讲记”");
        tipInfo.getStyleClass().add("field-info");
        //
        final HBox hBox = new HBox(label, album, tipInfo);
        hBox.setAlignment(Pos.CENTER_LEFT);
        this.form.getChildren().addAll(hBox);
    }

    private void edit_catalog() {
        final Label label = new Label("类目");
        label.getStyleClass().add("field-name");

        catalog = new TextArea("unknown");
        catalog.setPrefRowCount(3);
        HBox.setHgrow(catalog, Priority.ALWAYS);
        //
        catalog.setText(String.join("\n", document.getMetadata("catalog")));

        //
        final Label tipInfo = new Label("""
                每行表示一个，支持多级路径。如：
                般若注疏
                般若/注疏
                """);
        tipInfo.getStyleClass().add("field-info");
        //
        final HBox hBox = new HBox(label, catalog, tipInfo);
        hBox.setAlignment(Pos.CENTER_LEFT);
        this.form.getChildren().addAll(hBox);
    }

    private void edit_periods() {
        final Label label = new Label("年代");
        label.getStyleClass().add("field-name");

        periods = new TextArea("unknown");
        periods.setPrefRowCount(3);
        HBox.setHgrow(periods, Priority.ALWAYS);
        //
        periods.setText(String.join("\n", document.getMetadata("period")));

        //
        final Label tipInfo = new Label("""
                每行表示一个。如：
                唐
                宋
                """);
        tipInfo.getStyleClass().add("field-info");
        //
        final HBox hBox = new HBox(label, periods, tipInfo);
        hBox.setAlignment(Pos.CENTER_LEFT);
        this.form.getChildren().addAll(hBox);
    }

    private void edit_authors() {
        final Label label = new Label("作译者");
        label.getStyleClass().add("field-name");

        authors = new TextArea("unknown");
        authors.setPrefRowCount(4);
        HBox.setHgrow(authors, Priority.ALWAYS);
        //
        authors.setText(String.join("\n", document.getMetadata("author")));

        //
        final Label tipInfo = new Label("""
                每行表示一个。如：
                龙树菩萨 著
                鸠摩罗什 译
                """);
        tipInfo.getStyleClass().add("field-info");
        //
        final HBox hBox = new HBox(label, authors, tipInfo);
        hBox.setAlignment(Pos.CENTER_LEFT);
        this.form.getChildren().addAll(hBox);
    }

    private void edit_priority() {
        final Label label = new Label("优先级");
        label.getStyleClass().add("field-name");

        priority = new ChoiceBox<>();
        priority.getItems().setAll(IntStream.iterate(0, v -> v <= 10, v -> v + 1).boxed().collect(Collectors.toList()));
        //
        int val = NumberHelper.toInt(document.getMetadata("priority", "5"), 5);
        if (val < 0) val = 0;
        else if (val > 10) val = 10;
        priority.getSelectionModel().select(val);

        //
        final Label tipInfo = new Label("搜索结果显示优先级，0为最低，10为最高，5为默认");
        tipInfo.getStyleClass().add("field-info");
        //
        final HBox hBox = new HBox(label, priority, tipInfo);
        hBox.setAlignment(Pos.CENTER_LEFT);
        this.form.getChildren().addAll(hBox);
    }

    private void edit_indexable() {
        final Label label = new Label("搜索范围");
        label.getStyleClass().add("field-name");

        searchable = new ChoiceBox<>();
        searchable.getItems().setAll(Searchable.values());
        //
        searchable.getSelectionModel().select(document.getSearchable());

        //
        final Label tipInfo = new Label("搜索可见范围，默认为全部范围，若无必要不需修改");
        tipInfo.getStyleClass().add("field-info");
        //
        final HBox hBox = new HBox(label, searchable, tipInfo);
        hBox.setAlignment(Pos.CENTER_LEFT);
        this.form.getChildren().addAll(hBox);
    }

    public boolean showDialog() {
        final Dialog<?> dialog = new Dialog<>();
        dialog.setTitle("编辑".concat(document.item.provider.providerName()).concat("元数据"));
        dialog.setDialogPane(this);
        final BoolHolder result = new BoolHolder(false);
        dialog.initOwner(App.app().getPrimaryStage());
        dialog.showAndWait().ifPresent(v -> {
            result.value = true;
            //
            document.removeMetadata("library");
            library.getText().lines().map(String::strip).filter(s -> !s.isEmpty()).distinct().sorted()
                    .forEach(s -> document.addMetadata("library", s));
            document.removeMetadata("catalog");
            catalog.getText().lines().map(String::strip).filter(s -> !s.isEmpty()).distinct().sorted()
                    .forEach(s -> document.addMetadata("catalog", s));
            document.removeMetadata("album");
            album.getText().lines().map(String::strip).filter(s -> !s.isEmpty()).distinct().sorted()
                    .forEach(s -> document.addMetadata("album", s));
            document.removeMetadata("period");
            periods.getText().lines().map(String::strip).filter(s -> !s.isEmpty()).distinct().sorted()
                    .forEach(s -> document.addMetadata("period", s));
            document.removeMetadata("author");
            authors.getText().lines().map(String::strip).filter(s -> !s.isEmpty()).distinct().sorted()
                    .forEach(s -> document.addMetadata("author", s));
            //
            document.setMetadata("priority", priority.getSelectionModel().getSelectedItem().toString());
            //
            document.setSearchable(searchable.getSelectionModel().getSelectedItem());

            //
            App.app().toast("数据已修改，请手动保存生效！");
        });
        return result.value;
    }
}
