package org.appxi.smartlib.app.item.mindmap;

import javafx.scene.control.Button;
import javafx.scene.control.Tooltip;
import org.appxi.javafx.visual.MaterialIcon;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.smartlib.ItemEvent;
import org.appxi.smartlib.app.item.HtmlBasedViewer;
import org.appxi.smartlib.app.item.ItemEx;
import org.appxi.smartlib.app.recent.RecentViewSupport;

public class MindmapViewer extends MindmapRenderer implements RecentViewSupport {
    public MindmapViewer(WorkbenchPane workbench, ItemEx item) {
        super(workbench, item, true);

        HtmlBasedViewer.bindingViewer(this, item);
    }

    @Override
    public void deinitialize() {
        super.deinitialize();
        //
        app.eventBus.fireEvent(new ItemEvent(ItemEvent.VISITED, this.item));
    }

    @Override
    public void initialize() {
        super.initialize();
        //
        //addTool_ExportPng();
    }

    private void addTool_ExportPng() {
        final Button button = new Button("导出PNG");
        button.setDisable(true);
        button.setTooltip(new Tooltip("导出到PNG图片"));
        button.setGraphic(MaterialIcon.IMAGE.graphic());
        button.setOnAction(event -> {

        });
        this.webPane.getTopBar().addLeft(button);
    }

    @Override
    protected void onWebEngineLoadSucceeded() {
        super.onWebEngineLoadSucceeded();
        //
        app.eventBus.fireEvent(new ItemEvent(ItemEvent.VISITED, this.item));
    }
}
