package org.appxi.smartlib.html;

import javafx.beans.binding.Bindings;
import javafx.beans.property.IntegerProperty;
import javafx.beans.property.SimpleIntegerProperty;
import javafx.beans.value.ChangeListener;
import javafx.geometry.Orientation;
import javafx.geometry.Pos;
import javafx.scene.control.Button;
import javafx.scene.control.Separator;
import javafx.scene.control.TextField;
import javafx.scene.control.ToolBar;
import javafx.scene.control.Tooltip;
import javafx.scene.input.KeyCode;
import javafx.scene.input.KeyEvent;
import javafx.scene.layout.BorderPane;
import javafx.scene.layout.HBox;
import javafx.scene.layout.Priority;
import javafx.scene.layout.StackPane;
import javafx.scene.web.WebEngine;
import javafx.stage.FileChooser;
import netscape.javascript.JSObject;
import org.appxi.javafx.control.HTMLEditorEx;
import org.appxi.javafx.control.ProgressLayer;
import org.appxi.javafx.helper.FxHelper;
import org.appxi.javafx.visual.MaterialIcon;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.prefs.UserPrefs;
import org.appxi.smartlib.App;
import org.appxi.smartlib.item.Item;
import org.appxi.smartlib.item.ItemEvent;
import org.appxi.util.FileHelper;
import org.appxi.util.StringHelper;

import java.io.File;
import java.util.List;
import java.util.Objects;

public abstract class SimpleEditor extends HtmlEditor {
    protected final IntegerProperty documentChanges = new SimpleIntegerProperty(-1);
    protected HTMLEditorEx htmlEditor;
    private Button documentSaver;

    public SimpleEditor(Item item, WorkbenchPane workbench) {
        super(item, workbench);
    }

    @Override
    protected WebEngine webEngine() {
        if (null == this.webView) {
            this.webView = htmlEditor.webView();
            attachAdvancedPasteShortcuts(this.webView, () -> webView.isFocused());
        }
        return this.htmlEditor.webEngine();
    }

    @Override
    protected void onViewportInitOnce(StackPane viewport) {
        this.htmlEditor = new HTMLEditorEx();
        this.htmlEditor.setPickOnBounds(false);
        this.hackHtmlEditor();//

        HBox topArea = new HBox(8);
        topArea.setAlignment(Pos.CENTER_LEFT);
        topArea.setStyle("-fx-padding: .5em;");
        this.initTopArea(viewport, topArea);

        BorderPane borderPane = new BorderPane(htmlEditor);
        borderPane.setTop(topArea);
        viewport.getChildren().add(borderPane);
    }

    private void hackHtmlEditor() {
        htmlEditor.topToolbar().getStyleClass().add("flat");
        htmlEditor.bottomToolbar().getStyleClass().add("flat");
        this.hack_save(htmlEditor.topToolbar());
        this.hack_image(htmlEditor.topToolbar());
    }

    protected abstract void saveEditorContent(String content);

    protected abstract String loadEditorContent();

    private void hack_save(ToolBar toolBar) {
        documentSaver = new Button("保存");
        documentSaver.setGraphic(MaterialIcon.SAVE.graphic());
        documentSaver.setTooltip(new Tooltip("保存（Ctrl+S），非自动保存修改，请注意保存已修改的数据！"));
        documentSaver.setOnAction(event -> ProgressLayer.showAndWait(app.getPrimaryGlass(), progressLayer -> {
            saveEditorContent(this.htmlEditor.getHtmlText());
            FxHelper.runLater(() -> {
                documentChanges.set(0);
                app.toast("已保存");
                htmlEditor.webView().requestFocus();
            });
            //
            App.app().eventBus.fireEvent(new ItemEvent(ItemEvent.UPDATED, item));
        }));
        documentSaver.disableProperty().bind(Bindings.createBooleanBinding(() -> documentChanges.get() < 1, documentChanges));
        toolBar.getItems().addAll(0, List.of(documentSaver, new Separator(Orientation.VERTICAL)));
        //
        getViewport().addEventHandler(KeyEvent.KEY_RELEASED, event -> {
            if (event.isShortcutDown() && event.getCode() == KeyCode.S) {
                event.consume();
                documentSaver.fire();
            }
        });
    }

    private void hack_image(ToolBar toolBar) {
        final Button button = new Button("图片");
        button.setGraphic(MaterialIcon.ADD_PHOTO_ALTERNATE.graphic());
        button.setTooltip(new Tooltip("插入图片"));
        button.setOnAction(event -> {
            final String lastDir = UserPrefs.prefs.getString("chooser.image.dir", null);
            final FileChooser fileChooser = new FileChooser();
            if (null != lastDir)
                fileChooser.setInitialDirectory(new File(lastDir));
            fileChooser.setTitle("选择图片文件...");
            fileChooser.getExtensionFilters().add(new FileChooser.ExtensionFilter(
                    "Image", "*.jpg", "*.png", "*.jpeg", "*.gif"));
            final File file = fileChooser.showOpenDialog(app.getPrimaryStage());
            if (null == file)
                return;
            UserPrefs.prefs.setProperty("chooser.image.dir", file.getParent());
            //
            insertImageFileAtCursor(file);
        });
        toolBar.getItems().addAll(2, List.of(button, new Separator(Orientation.VERTICAL)));
    }

    protected void insertHtmlAtCursor(String html) {
        webEngine().executeScript("""
                function insertHtmlAtCursor(html) {
                    if (window.getSelection && window.getSelection().getRangeAt) {
                        const range = window.getSelection().getRangeAt(0);
                        const node = range.createContextualFragment(html);
                        range.insertNode(node);
                    } else if (document.selection && document.selection.createRange) {
                        document.selection.createRange().pasteHTML(html);
                    }
                }insertHtmlAtCursor('"""
                .concat(Objects.requireNonNull(HtmlHelper.escapeJavaStyleString(html, true, true)))
                .concat("')"));
    }

    protected void initTopArea(StackPane viewport, HBox topArea) {
        edit_NameEdit(topArea);
        edit_Metadata(topArea);
    }

    private ChangeListener<String> nameChangeListener;

    private void edit_NameEdit(HBox topArea) {
        final TextField nameEditor = new TextField(item.getName());
        nameEditor.setPromptText("在此输入");
        nameEditor.setTooltip(new Tooltip("在此处输入后按回车应用修改"));
        HBox.setHgrow(nameEditor, Priority.ALWAYS);
        nameChangeListener = (o, ov, nv) -> nameEditor.setText(nv);
        item.name.addListener(nameChangeListener);
        nameEditor.addEventHandler(KeyEvent.KEY_RELEASED, event -> {
            if (event.getCode() == KeyCode.ENTER) {
                event.consume();
                String inputText = FileHelper.toValidName(nameEditor.getText()).strip().replaceAll("^[.]", "");
                if (inputText.isBlank()) {
                    nameEditor.setText(item.getName());
                    return;
                }
                String targetName = StringHelper.trimChars(inputText, 80);
                if (targetName.equals(item.getName()))
                    return;

                final String newName = targetName;
                ProgressLayer.showAndWait(getViewport(), progressLayer -> {
                    final Item oldItem = item.clone(), newItem = item.clone();
                    final String msg = org.appxi.smartlib.dao.DataApi.dataAccess().rename(newItem, newName);
                    if (msg != null) {
                        app.toastError(msg);
                        return;
                    }
                    FxHelper.runLater(() -> {
                        item.setName(newItem.getName()).setPath(newItem.getPath());
                        //
                        App.app().eventBus.fireEvent(new ItemEvent(ItemEvent.RENAMED, item, oldItem));
                        app.toast("重命名成功");
                    });
                });
            }
        });
        topArea.getChildren().addAll(nameEditor);
    }

    private void edit_Metadata(HBox topArea) {
        final Button button = new Button("元数据");
        button.setTooltip(new Tooltip("编辑此文档的类目、作者等信息"));
        button.setGraphic(MaterialIcon.STYLE.graphic());
        button.setOnAction(event -> editMetadata());
        topArea.getChildren().add(button);
    }

    protected abstract void editMetadata();

    @Override
    protected void onWebEngineLoading() {
        String script = """
                <script type="text/javascript">
                    var javaApp;
                    function attachDocumentChangedListener() {
                        MutationObserver = window.MutationObserver || window.WebKitMutationObserver;
                        var observer = new MutationObserver(function(mutations, observer) {
                            javaApp.updateDocumentChanges();
                        });
                        observer.observe(document, {
                            childList: true,
                            subtree: true,
                            attributes: true,
                            characterData: true
                        });
                    }
                </script>
                """;
        String template = "<html><head>"
                .concat(script)
                .concat("</head>")
                .concat(loadEditorContent())
                .concat("</html>");
        this.htmlEditor.setHtmlText(template);
    }

    @Override
    protected void onWebEngineLoadSucceeded() {
        // set an interface object named 'javaApp' in the web engine's page
        final JSObject window = (JSObject) webEngine().executeScript("window");
        window.setMember("javaApp", javaApp);

        webEngine().executeScript("attachDocumentChangedListener()");

        super.onWebEngineLoadSucceeded();
    }

    @Override
    public void onViewportClosing(boolean selected) {
        super.onViewportClosing(selected);
        if (null != this.nameChangeListener)
            item.name.removeListener(this.nameChangeListener);
//        if (null != documentSaver && documentChanges.get() > 0) {
//            Alert alert = new Alert(Alert.AlertType.CONFIRMATION);
//            alert.setTitle(this.viewTitle.get());
//            alert.setHeaderText("文档已修改");
//            alert.setContentText("是否保存已修改的文档？");
//            alert.setHeight(200);
//            FxHelper.withTheme(getApplication(), alert).showAndWait().filter(v -> v == ButtonType.OK)
//                    .ifPresent(v -> documentSaver.fire());
//        }
    }

    @Override
    protected void saveUserExperienceData() {
    }

    ////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

    /**
     * for communication from the Javascript engine.
     */
    private final JavaApp javaApp = new JavaApp();

    public final class JavaApp {
        private JavaApp() {
        }

        public void log(String text) {
            System.out.println("javaApp > ".concat(text));
        }

        public void updateDocumentChanges() {
            documentChanges.set(documentChanges.get() + 1);
        }
    }
}
