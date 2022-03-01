package org.appxi.smartlib.item.mindmap;

import javafx.event.Event;
import javafx.scene.control.Button;
import javafx.scene.control.Tooltip;
import javafx.scene.layout.StackPane;
import org.appxi.javafx.visual.MaterialIcon;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.smartlib.item.Item;
import org.appxi.smartlib.item.ItemEvent;
import org.appxi.smartlib.recent.RecentViewSupport;

class MindmapViewer extends MindmapRenderer implements RecentViewSupport {
    public MindmapViewer(Item item, WorkbenchPane workbench) {
        super(item, workbench, false);
    }

    @Override
    protected void onViewportInitOnce(StackPane viewport) {
        super.onViewportInitOnce(viewport);
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
        webPane().getTopAsBar().addLeft(button);
    }

    @Override
    protected void onWebEngineLoadSucceeded() {
        super.onWebEngineLoadSucceeded();
        //
        app.eventBus.fireEvent(new ItemEvent(ItemEvent.VISITED, this.item));
    }

    @Override
    public void onViewportClosing(Event event, boolean selected) {
        super.onViewportClosing(event, selected);
        //
        app.eventBus.fireEvent(new ItemEvent(ItemEvent.VISITED, this.item));
    }
}
