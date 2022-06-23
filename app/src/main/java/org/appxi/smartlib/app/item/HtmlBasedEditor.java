package org.appxi.smartlib.app.item;

import javafx.scene.input.KeyEvent;
import org.appxi.javafx.app.DesktopApp;
import org.appxi.javafx.control.ProgressLayer;
import org.appxi.javafx.helper.FxHelper;
import org.appxi.javafx.visual.VisualEvent;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.smartlib.ItemEvent;
import org.appxi.smartlib.app.App;
import org.appxi.smartlib.html.HtmlHelper;

public abstract class HtmlBasedEditor extends WebBasedEditor {
    private static final Object AK_INITIALIZED = new Object();

    public HtmlBasedEditor(WorkbenchPane workbench, ItemEx item) {
        super(workbench, item);
    }


    @Override
    protected void insertHtmlAtCursor(String html) {
        webPane.executeScript("tinymce.activeEditor.insertContent('"
                .concat(HtmlHelper.escapeJavaStyleString(html, true, false))
                .concat("')"));
    }

    /* //////////////////////////////////////////////////////////////////// */

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
        if (!webPane.getProperties().containsKey(AK_INITIALIZED)) {
            webPane.getProperties().put(AK_INITIALIZED, true);
            webPane.webView().addEventFilter(KeyEvent.KEY_PRESSED,
                    e -> this.handleAdvancedPaste(e, () -> this.webPane.executeScript("tinymce.activeEditor.hasFocus()")));
        }
        //
        super.onWebEngineLoadSucceeded();
        //
        if (_cachedEditorIsDirty) {
            webPane.executeScript("setTimeout(function(){tinymce.activeEditor.setDirty(true)}, 60)");
        }
    }

    @Override
    protected void onAppStyleSetting(VisualEvent event) {
        // 缓存编辑状态
        _cachedEditorIsDirty = webPane.executeScript("tinymce.activeEditor.isDirty()");
        // 缓存编辑数据
        _cachedEditorContent = webPane.executeScript("tinymce.activeEditor.getContent()");
        // 重新加载/构建编辑器以应用新的配色
        super.navigating(null, false);
    }
    ////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

    public class WebCallbackImpl extends WebCallback {
        public String initEditor() {
            String editorContent = _cachedEditorContent != null ? _cachedEditorContent : loadEditorContent();
            _cachedEditorContent = null;
            return editorContent;
        }

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
