package org.appxi.smartlib.item.mindmap;

import netscape.javascript.JSObject;
import org.appxi.javafx.app.DesktopApp;
import org.appxi.javafx.visual.VisualEvent;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.smartlib.html.HtmlRendererEx;
import org.appxi.smartlib.item.Item;

abstract class MindmapRenderer extends HtmlRendererEx {
    final MindmapDocument document;

    public MindmapRenderer(Item item, WorkbenchPane workbench, boolean editing) {
        super(item, workbench, editing);
        this.document = new MindmapDocument(item);
    }

    @Override
    public void navigate(Item item) {
        //TODO
    }

    @Override
    protected final void applyWebStyle(VisualEvent event) {
        //TODO
    }

    @Override
    protected void onWebEngineLoading() {
        webPane().webEngine().load(DesktopApp.appDir().resolve("template/mindmap/dist/" + (editing ? "editor" : "viewer") + ".html").toUri().toString());
    }

    @Override
    protected void onWebEngineLoadSucceeded() {
        // set an interface object named 'javaApp' in the web engine's page
        final JSObject window = webPane().executeScript("window");
        window.setMember("javaApp", javaApp);
        //
        super.onWebEngineLoadSucceeded();
        //
        webPane().webView().setContextMenuEnabled(false);
    }

    ////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

    /**
     * for communication from the Javascript engine.
     */
    private final JavaApp javaApp = new JavaApp();

    public final class JavaApp {
        public void initEditor() {
            webPane().executeScript("minder.importJson(".concat(document.getDocument().toString()).concat(")"));
        }
    }
}
