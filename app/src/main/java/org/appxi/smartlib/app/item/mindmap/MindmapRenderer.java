package org.appxi.smartlib.app.item.mindmap;

import org.appxi.javafx.app.DesktopApp;
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
    public void postConstruct() {
    }

    @Override
    protected void navigating(Object location, boolean firstTime) {
        // 目前未实现定位功能，仅首次时加载内容
        if (firstTime) {
            super.navigating(location, true);
        }
    }

    @Override
    protected WebCallbackImpl createWebCallback() {
        return new WebCallbackImpl();
    }

    @Override
    protected final Object createWebContent() {
        return DesktopApp.appDir().resolve("template/mindmap/dist/" + (readonly ? "viewer" : "editor") + ".html");
    }

    public class WebCallbackImpl extends WebCallback {
        public String initEditor() {
            return document.getDocument().toString();
        }
    }
}
