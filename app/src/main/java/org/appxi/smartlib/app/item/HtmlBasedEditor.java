package org.appxi.smartlib.app.item;

import javafx.scene.input.KeyEvent;
import org.appxi.javafx.app.DesktopApp;
import org.appxi.javafx.control.ProgressLayer;
import org.appxi.javafx.visual.VisualEvent;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.smartlib.ItemEvent;
import org.appxi.smartlib.app.App;
import org.appxi.smartlib.html.HtmlHelper;
import org.appxi.util.ext.RawVal;

import java.util.List;

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
    protected WebJavaBridge createWebJavaBridge() {
        return new WebJavaBridgeImpl();
    }

    protected abstract String loadEditorContent();

    protected abstract void saveEditorContent(String content) throws Exception;

    @Override
    protected void onWebEngineLoadSucceeded() {
        if (!webPane.getProperties().containsKey(AK_INITIALIZED)) {
            webPane.getProperties().put(AK_INITIALIZED, true);
            webPane.webView().addEventFilter(KeyEvent.KEY_PRESSED,
                    e -> this.handleAdvancedPaste(e, () -> this.webPane.executeScript("tinymce.activeEditor.hasFocus()")));
        }
        //
        super.onWebEngineLoadSucceeded();
    }

    @Override
    protected void onAppStyleSetting(VisualEvent event) {
        webPane.executeScript("typeof(setWebStyleTheme) === 'function' && setWebStyleTheme('" + app.visualProvider + "');");
    }
    ////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

    public class WebJavaBridgeImpl extends WebJavaBridge {
        @Override
        protected List<RawVal<Object>> getJavaReadyArguments() {
            List<RawVal<Object>> args = super.getJavaReadyArguments();
            args.add(RawVal.kv("content", loadEditorContent()));
            return args;
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
    }
}
