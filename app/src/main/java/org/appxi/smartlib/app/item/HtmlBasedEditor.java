package org.appxi.smartlib.app.item;

import javafx.application.Platform;
import javafx.scene.input.Clipboard;
import javafx.scene.input.DataFormat;
import javafx.scene.layout.StackPane;
import javafx.scene.web.WebEngine;
import javafx.scene.web.WebView;
import org.appxi.javafx.app.DesktopApp;
import org.appxi.javafx.control.ProgressLayer;
import org.appxi.javafx.helper.FxHelper;
import org.appxi.javafx.visual.VisualEvent;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.smartlib.ItemEvent;
import org.appxi.smartlib.app.App;
import org.appxi.smartlib.html.HtmlHelper;

import java.net.URI;
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
        URI uri = DesktopApp.appDir().resolve("template/tinymce/editor.html").toUri();
        try {
            return new URI(uri + "?darkMode=" + app.visualProvider.theme().name().contains("DARK")
            );
        } catch (Exception e) {
            return uri;
        }
    }

    @Override
    protected WebCallback createWebCallback() {
        return new WebCallbackImpl();
    }

    protected abstract String loadEditorContent();

    protected abstract void saveEditorContent(String content) throws Exception;

    private String _cachedEditorContent;
    private boolean _cachedEditorIsDirty;

    @Override
    protected void onWebEngineLoadSucceeded() {
        super.onWebEngineLoadSucceeded();
        //
        String editorContent = _cachedEditorContent != null ? _cachedEditorContent : loadEditorContent();
        _cachedEditorContent = null;
        if (null != editorContent && editorContent.length() > 0) {
            webEngine().executeScript("setTimeout(function(){tinymce.activeEditor.setContent('"
                    .concat(HtmlHelper.escapeJavaStyleString(editorContent, true, false))
                    .concat("')}, 50)"));
        }
        if (_cachedEditorIsDirty) {
            webEngine().executeScript("setTimeout(function(){tinymce.activeEditor.setDirty(true)}, 60)");
        }
    }

    @Override
    protected void onAppStyleSetting(VisualEvent event) {
        FxHelper.runLater(() -> {
            _cachedEditorIsDirty = (boolean) webEngine().executeScript("tinymce.activeEditor.isDirty()");
            _cachedEditorContent = (String) webEngine().executeScript("tinymce.activeEditor.getContent()");
            super.navigating(null, false);
            super.onAppStyleSetting(event);
        });
    }
    ////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

    public class WebCallbackImpl extends WebCallback {
        public void saveEditor(String content) {
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
