package org.appxi.smartlib.app.item;

import javafx.scene.web.WebEngine;
import javafx.scene.web.WebView;
import org.appxi.javafx.app.DesktopApp;
import org.appxi.javafx.app.web.WebViewer;
import org.appxi.javafx.control.ProgressLayer;
import org.appxi.javafx.helper.FxHelper;
import org.appxi.javafx.visual.VisualEvent;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.smartlib.ItemEvent;
import org.appxi.smartlib.app.App;
import org.appxi.smartlib.html.HtmlHelper;
import org.appxi.util.FileHelper;

import java.nio.charset.StandardCharsets;
import java.util.Base64;
import java.util.Optional;

public abstract class HtmlBasedEditor extends WebBasedEditor {
    public HtmlBasedEditor(WorkbenchPane workbench, ItemEx item) {
        super(workbench, item);
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
    protected void navigating(Object location, boolean firstTime) {
        if (firstTime) {
            // 暂时以此方法特殊处理，可能tinymce的编辑器太特殊，导致直接在html中引入css文件也无效果
            String cssStr = """
                    :root {
                        --font-family: tibetan, "%s", AUTO !important;
                        --zoom: %.2f !important;
                        --text-color: %s;
                    }
                    body {
                        background-color: %s;
                    }
                    """.formatted(
                    app.visualProvider.webFontName(),
                    app.visualProvider.webFontSize(),
                    app.visualProvider.webTextColor(),
                    app.visualProvider.webPageColor()
            );
            cssStr += Optional.ofNullable(WebViewer.getWebIncludeDir())
                    .map(dir -> FileHelper.readString(dir.resolve("app-base.css")))
                    .orElse("");

            // 编码成base64并应用
            String cssData = "data:text/css;charset=utf-8;base64,"
                             + Base64.getMimeEncoder().encodeToString(cssStr.getBytes(StandardCharsets.UTF_8));
            FxHelper.runLater(() -> {
                webPane.webEngine().setUserStyleSheetLocation(cssData);
                webPane.executeScript("document.body.setAttribute('class','" + app.visualProvider + "');");
            });
        }
        super.navigating(location, firstTime);
    }

    @Override
    protected Object createWebContent() {
        return DesktopApp.appDir().resolve("template/tinymce/editor.html");
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
            FxHelper.copyText(text);
        }
    }
}
