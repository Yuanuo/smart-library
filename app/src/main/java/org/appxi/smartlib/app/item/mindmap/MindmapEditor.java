package org.appxi.smartlib.app.item.mindmap;

import javafx.scene.control.Button;
import javafx.scene.control.Tooltip;
import javafx.scene.layout.StackPane;
import org.appxi.javafx.control.ProgressLayer;
import org.appxi.javafx.helper.FxHelper;
import org.appxi.javafx.visual.MaterialIcon;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.smartlib.Item;
import org.appxi.smartlib.ItemEvent;
import org.appxi.smartlib.app.App;

public class MindmapEditor extends MindmapRenderer {
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
            try {
                document.save();
            } catch (Exception e) {
                app.toastError(e.getMessage());
                return;
            }
            //
            app.toast("已保存");
            App.app().eventBus.fireEvent(new ItemEvent(ItemEvent.UPDATED, item));
        })));
        webPane().getTopAsBox().addRight(button);
    }
}
