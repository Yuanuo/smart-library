package org.appxi.smartlib.item;

import javafx.beans.binding.Bindings;
import org.appxi.javafx.workbench.WorkbenchPane;

public abstract class ItemEditor extends ItemController {

    public ItemEditor(Item item, WorkbenchPane workbench) {
        super(item, workbench);
        //
        this.id.bind(Bindings.createStringBinding(() -> "Edit@".concat(item.typedPath()), item.path));
        //
        this.title.bind(Bindings.createStringBinding(() -> "编辑: ".concat(item.getName()), item.name));
        this.tooltip.bind(Bindings.createStringBinding(() -> "编辑: ".concat(item.typedPath()), item.path));
    }
}
