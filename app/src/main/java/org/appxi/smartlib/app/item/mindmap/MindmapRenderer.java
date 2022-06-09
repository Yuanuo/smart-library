package org.appxi.smartlib.app.item.mindmap;

import org.appxi.javafx.app.DesktopApp;
import org.appxi.javafx.app.web.WebCallback;
import org.appxi.javafx.app.web.WebRenderer;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.smartlib.app.item.ItemEx;
import org.appxi.smartlib.app.item.ItemRenderer;
import org.appxi.smartlib.app.item.WebBasedEditor;
import org.appxi.smartlib.mindmap.MindmapDocument;

abstract class MindmapRenderer extends WebBasedEditor implements ItemRenderer {
    final boolean readonly;
    final MindmapDocument document;

    public MindmapRenderer(WorkbenchPane workbench, ItemEx item, boolean readonly) {
        super(workbench, null, item);
        this.readonly = readonly;
        this.document = new MindmapDocument(item);
    }

    @Override
    public void initialize() {
    }

    @Override
    protected void navigating(Object location, boolean firstTime) {
        webPane().webEngine().load(DesktopApp.appDir().resolve("template/mindmap/dist/" + (readonly ? "viewer" : "editor") + ".html").toUri().toString());
    }

    @Override
    protected void onWebEngineLoadSucceeded() {
        super.onWebEngineLoadSucceeded();
        //
        webPane().webView().setContextMenuEnabled(false);
    }

    @Override
    protected WebCallback createWebCallback() {
        return new WebCallbackImpl(this);
    }

    @Override
    protected final Object createWebContent() {
        throw new UnsupportedOperationException("Should never happen");
    }

    ////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

    public class WebCallbackImpl extends WebCallback {
        public WebCallbackImpl(WebRenderer webRenderer) {
            super(webRenderer);
        }

        public void initEditor() {
            webPane().executeScript("minder.importJson(".concat(document.getDocument().toString()).concat(")"));
        }
    }
}
