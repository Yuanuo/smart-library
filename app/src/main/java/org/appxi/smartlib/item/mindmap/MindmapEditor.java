package org.appxi.smartlib.item.mindmap;

import javafx.scene.control.Button;
import javafx.scene.control.Tooltip;
import javafx.scene.layout.StackPane;
import org.appxi.javafx.control.ProgressLayer;
import org.appxi.javafx.helper.FxHelper;
import org.appxi.javafx.visual.MaterialIcon;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.smartlib.App;
import org.appxi.smartlib.item.Item;
import org.appxi.smartlib.item.ItemEvent;

class MindmapEditor extends MindmapRenderer {
    public MindmapEditor(Item item, WorkbenchPane workbench) {
        super(item, workbench, true);
    }

    @Override
    protected void onViewportInitOnce(StackPane viewport) {
        super.onViewportInitOnce(viewport);
        //
        addEdit_Metadata(document);
        addEdit_Save();
    }

    private void addEdit_Save() {
        final Button button = new Button("保存");
        button.setTooltip(new Tooltip("保存修改（Ctrl + S）"));
        button.setGraphic(MaterialIcon.SAVE.graphic());
        button.setOnAction(event -> ProgressLayer.showAndWait(app.getPrimaryGlass(), progressLayer -> FxHelper.runLater(() -> {
            String json = webPane().executeScript("JSON.stringify(minder.exportJson())");
            document.setDocumentBody(json);
            document.save();
            //
            app.toast("已保存");
            App.app().eventBus.fireEvent(new ItemEvent(ItemEvent.UPDATED, item));
        })));
        webPane().getTopAsBox().addRight(button);
    }
}
