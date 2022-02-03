package org.appxi.smartlib.html;

import javafx.application.Platform;
import javafx.beans.value.ChangeListener;
import javafx.event.Event;
import javafx.geometry.Pos;
import javafx.scene.control.Button;
import javafx.scene.control.TextField;
import javafx.scene.control.Tooltip;
import javafx.scene.input.Clipboard;
import javafx.scene.input.DataFormat;
import javafx.scene.input.KeyCode;
import javafx.scene.input.KeyEvent;
import javafx.scene.layout.HBox;
import javafx.scene.layout.Priority;
import javafx.scene.layout.StackPane;
import javafx.scene.web.WebEngine;
import netscape.javascript.JSObject;
import org.appxi.javafx.app.DesktopApp;
import org.appxi.javafx.control.ProgressLayer;
import org.appxi.javafx.control.WebPane;
import org.appxi.javafx.helper.FxHelper;
import org.appxi.javafx.visual.MaterialIcon;
import org.appxi.javafx.visual.VisualEvent;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.smartlib.App;
import org.appxi.smartlib.item.Item;
import org.appxi.smartlib.item.ItemEvent;
import org.appxi.util.FileHelper;
import org.appxi.util.StringHelper;

import java.nio.file.Path;
import java.util.Map;

public abstract class AdvancedEditor extends HtmlEditor {
    WebPane webPane;

    public AdvancedEditor(Item item, WorkbenchPane workbench) {
        super(item, workbench);
    }

    @Override
    protected WebEngine webEngine() {
        if (null == this.webView) {
            this.webView = this.webPane.webView();
            attachAdvancedPasteShortcuts(this.webView, () -> webPane.executeScript("tinymce.activeEditor.hasFocus()"));
        }
        return this.webPane.webEngine();
    }

    @Override
    protected void insertHtmlAtCursor(String html) {
        webEngine().executeScript("tinymce.activeEditor.insertContent('"
                .concat(HtmlHelper.escapeJavaStyleString(html, true, false))
                .concat("')"));
    }

    @Override
    protected void onViewportInitOnce(StackPane viewport) {
        this.webPane = new WebPane();

        HBox topArea = new HBox(8);
        topArea.setAlignment(Pos.CENTER_LEFT);
        topArea.setStyle("-fx-padding: .5em;");
        this.initTopArea(viewport, topArea);

        viewport.getChildren().add(this.webPane);
    }

    protected void initTopArea(StackPane viewport, HBox topArea) {
        this.webPane.setTop(topArea);
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

                final Item oldItem = item.clone(), newItem = item.clone();
                final String newName = targetName;
                ProgressLayer.showAndWait(getViewport(), progressLayer -> {
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
    /* //////////////////////////////////////////////////////////////////// */

    @Override
    protected void onWebEngineLoading() {
        loadDefaultEditor();
    }

    protected void loadDefaultEditor() {
        String template = """
                <!DOCTYPE html><html lang="zh"><head><meta charset="UTF-8">
                <link rel="stylesheet" href="${webIncl}"/>
                <style>
                /* For other boilerplate styles, see: /docs/general-configuration-guide/boilerplate-content-css/ */
                /*
                * For rendering images inserted using the image plugin.
                * Includes image captions using the HTML5 figure element.
                */
                figure.image {
                  display: inline-block;
                  border: 1px solid gray;
                  margin: 0 2px 0 1px;
                  background: #f5f2f0;
                }
                figure.align-left {
                  float: left;
                }
                figure.align-right {
                  float: right;
                }
                figure.image img {
                  margin: 8px 8px 0 8px;
                }
                figure.image figcaption {
                  margin: 6px 8px 6px 8px;
                  text-align: center;
                }

                /*
                 Alignment using classes rather than inline styles
                 check out the "formats" option
                */
                img.align-left {
                  float: left;
                }
                img.align-right {
                  float: right;
                }
                </style>
                <script src='${tinymce}'></script>
                <script type="text/javascript">
                var useDarkMode = ${useDarkMode};

                tinymce.init({
                  selector: 'body > textarea#editor',
                  language: 'zh_CN',
                  branding: false,
                  plugins: 'preview paste importcss searchreplace autolink autoresize save code visualblocks visualchars fullscreen image table charmap hr pagebreak nonbreaking anchor insertdatetime advlist lists wordcount imagetools textpattern noneditable help charmap quickbars emoticons footnotes',
                  imagetools_cors_hosts: ['picsum.photos'],
                  menubar: 'file edit view insert format tools table help',
                  toolbar: 'save | undo redo | bold italic underline strikethrough | formatselect | alignleft aligncenter alignright alignjustify | outdent indent lineheight |  numlist bullist | forecolor backcolor removeformat | pagebreak | charmap emoticons | preview | blockquote anchor footnotes',
                  toolbar_sticky: true,
                  image_advtab: true,
                  image_caption: true,
                  paste_data_images: true,
                  images_dataimg_filter: function (imgItm) { return false; },
                  link_list: [],
                  image_list: [],
                  image_class_list: [],
                  importcss_append: true,
                  file_picker_callback: function (callback, value, meta) {},
                  templates: [],
                  template_cdate_format: '[Date Created (CDATE): %m/%d/%Y : %H:%M:%S]',
                  template_mdate_format: '[Date Modified (MDATE): %m/%d/%Y : %H:%M:%S]',
                  quickbars_selection_toolbar: 'bold italic | quicklink h2 h3 blockquote quickimage quicktable',
                  noneditable_noneditable_class: 'mceNonEditable',
                  toolbar_mode: 'sliding',
                  contextmenu: 'link image imagetools table',
                  skin: useDarkMode ? 'oxide-dark' : 'oxide',
                  content_css: useDarkMode ? 'dark' : 'default',
                  content_style: 'a.mce-item-anchor[id^=a-]{display: none} a.mce-item-anchor[data-note]{background:unset;width:auto !important;height:auto !important;} a.mce-item-anchor[data-note]:before{content:"✱";font-size:85%}',
                  pagebreak_separator: '<p data-pb></p>',
                  pagebreak_split_block: true,
                  fontsize_formats: '0.8rem 0.9rem 1.0rem 1.1rem 1.2rem 1.3rem 1.4rem 1.5rem 1.6rem 1.7rem 1.8rem 2.0rem 2.2rem',
                  lineheight_formats: '1 1.1 1.2 1.3 1.4 1.5 2',
                  init_instance_callback: function(editor) { editor.execCommand('mceFullScreen'); },
                  urlconverter_callback: function(url, node, on_save, name) { return url; },
                  save_onsavecallback: function(editor) {
                    /*javaApp.log('save...');*/
                    javaApp.save(editor.getContent());
                  },
                 });
                </script>
                </head>
                <body><textarea id="editor"></textarea></body></html>
                """;
        template = template.replace("${webIncl}", WebIncl.webIncl)
                .replace("${tinymce}", WebIncl.tinymce)
                .replace("${useDarkMode}", String.valueOf(app.visualProvider.theme().name().contains("DARK")))
        ;
        webEngine().loadContent(template);
    }

    protected abstract String loadEditorContent();

    protected abstract void saveEditorContent(String content);

    private String cachedEditorContent;
    private boolean cachedEditorIsDirty;

    @Override
    protected void onWebEngineLoadSucceeded() {
        // set an interface object named 'javaApp' in the web engine's page
        final JSObject window = (JSObject) webEngine().executeScript("window");
        window.setMember("javaApp", javaApp);

        webView.setContextMenuEnabled(false);

        String editorContent = cachedEditorContent != null ? cachedEditorContent : loadEditorContent();
        cachedEditorContent = null;
        if (null != editorContent && editorContent.length() > 0)
            webEngine().executeScript("setTimeout(function(){tinymce.activeEditor.setContent('"
                    .concat(HtmlHelper.escapeJavaStyleString(editorContent, true, false))
                    .concat("')}, 100)"));
        if (cachedEditorIsDirty)
            webEngine().executeScript("setTimeout(function(){tinymce.activeEditor.setDirty(true)}, 120)");

        super.onWebEngineLoadSucceeded();
    }

    @Override
    protected void onSetAppStyle(VisualEvent event) {
        if (null == this.webView) return;
        FxHelper.runLater(() -> {
            cachedEditorIsDirty = (boolean) webEngine().executeScript("tinymce.activeEditor.isDirty()");
            cachedEditorContent = (String) webEngine().executeScript("tinymce.activeEditor.getContent()");
            loadDefaultEditor();
            super.onSetAppStyle(event);
        });
    }

    @Override
    public void onViewportClosing(Event event, boolean selected) {
        super.onViewportClosing(event, selected);
        if (null != webPane) webPane.release();
        if (null != this.nameChangeListener)
            item.name.removeListener(this.nameChangeListener);
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

        public void save(String content) {
            ProgressLayer.showAndWait(app.getPrimaryGlass(), progressLayer -> {
                saveEditorContent(content);
                app.toast("已保存");
                //
                App.app().eventBus.fireEvent(new ItemEvent(ItemEvent.UPDATED, item));
            });
        }

        public void setClipboardText(String text) {
//            System.out.println(mime + " >>> " + data);
            Platform.runLater(() -> Clipboard.getSystemClipboard().setContent(Map.of(DataFormat.PLAIN_TEXT, text)));
        }
    }

    private final static class WebIncl {
        static final Path template = DesktopApp.appDir().resolve("template");
        static final String webIncl = template.resolve("web-incl/app.css").toUri().toString();
        static final String tinymce = template.resolve("tinymce/tinymce.js").toUri().toString();
    }
}
