package org.appxi.smartlib.app.item;

import javafx.application.Platform;
import javafx.scene.input.Clipboard;
import javafx.scene.input.DataFormat;
import javafx.scene.layout.StackPane;
import javafx.scene.web.WebEngine;
import javafx.scene.web.WebView;
import org.appxi.javafx.app.DesktopApp;
import org.appxi.javafx.app.web.WebCallback;
import org.appxi.javafx.app.web.WebRenderer;
import org.appxi.javafx.control.ProgressLayer;
import org.appxi.javafx.helper.FxHelper;
import org.appxi.javafx.visual.VisualEvent;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.smartlib.ItemEvent;
import org.appxi.smartlib.app.App;
import org.appxi.smartlib.html.HtmlHelper;

import java.util.Map;

public abstract class HtmlBasedEditor extends WebBasedEditor {
    public HtmlBasedEditor(WorkbenchPane workbench, StackPane viewport, ItemEx item) {
        super(workbench, viewport, item);
    }

    private WebView cachedWebView;

    private WebEngine webEngine() {
        if (null == this.cachedWebView) {
            this.cachedWebView = this.webPane.webView();
            attachAdvancedPasteShortcuts(this.cachedWebView, () -> this.webPane.executeScript("tinymce.activeEditor.hasFocus()"));
        }
        return this.webPane.webEngine();
    }

    @Override
    protected void insertHtmlAtCursor(String html) {
        webEngine().executeScript("tinymce.activeEditor.insertContent('"
                .concat(HtmlHelper.escapeJavaStyleString(html, true, false))
                .concat("')"));
    }

    /* //////////////////////////////////////////////////////////////////// */

    @Override
    protected Object createWebContent() {
        String template = """
                <!DOCTYPE html><html lang="zh"><head><meta charset="UTF-8">
                <link rel="stylesheet" href="${appDir}template/web-incl/app.css"/>
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
                <script src='${appDir}template/tinymce/tinymce.js'></script>
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
                    /**/console.log('save...');
                    javaApp.save(editor.getContent());
                  },
                 });
                </script>
                </head>
                <body><textarea id="editor"></textarea></body></html>
                """;
        template = template.replace("${appDir}", DesktopApp.appDir().toUri().toString())
                .replace("${useDarkMode}", String.valueOf(app.visualProvider.theme().name().contains("DARK")))
        ;
        return template;
    }

    @Override
    protected WebCallback createWebCallback() {
        return new WebCallbackImpl(this);
    }

    protected abstract String loadEditorContent();

    protected abstract void saveEditorContent(String content) throws Exception;

    private String cachedEditorContent;
    private boolean cachedEditorIsDirty;

    @Override
    protected void onWebEngineLoadSucceeded() {
        super.onWebEngineLoadSucceeded();
        //
        this.webPane.webView().setContextMenuEnabled(false);

        String editorContent = cachedEditorContent != null ? cachedEditorContent : loadEditorContent();
        cachedEditorContent = null;
        if (null != editorContent && editorContent.length() > 0)
            webEngine().executeScript("setTimeout(function(){tinymce.activeEditor.setContent('"
                    .concat(HtmlHelper.escapeJavaStyleString(editorContent, true, false))
                    .concat("')}, 100)"));
        if (cachedEditorIsDirty)
            webEngine().executeScript("setTimeout(function(){tinymce.activeEditor.setDirty(true)}, 120)");
    }

    @Override
    protected void onAppStyleSetting(VisualEvent event) {
        if (null == this.webPane) return;
        FxHelper.runLater(() -> {
            cachedEditorIsDirty = (boolean) webEngine().executeScript("tinymce.activeEditor.isDirty()");
            cachedEditorContent = (String) webEngine().executeScript("tinymce.activeEditor.getContent()");
            super.navigating(null, false);
            super.onAppStyleSetting(event);
        });
    }
    ////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

    public class WebCallbackImpl extends WebCallback {
        public WebCallbackImpl(WebRenderer webRenderer) {
            super(webRenderer);
        }

        public void save(String content) {
            ProgressLayer.showAndWait(app.getPrimaryGlass(), progressLayer -> {
                try {
                    saveEditorContent(content);
                } catch (Exception e) {
                    app.toastError(e.getMessage());
                    return;
                }
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
}
