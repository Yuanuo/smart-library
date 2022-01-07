package org.appxi.smartlib.item;

import javafx.beans.binding.Bindings;
import org.appxi.javafx.workbench.WorkbenchPane;

public abstract class ItemEditor extends ItemController {

    public ItemEditor(Item item, WorkbenchPane workbench) {
        super(item, workbench);
        //
        this.viewId.bind(Bindings.concat("Edit@", item.path));
        //
        this.viewTitle.bind(Bindings.concat("编辑: ", item.name));
        this.viewTooltip.bind(Bindings.concat("编辑: ", item.typedPath()));
        this.appTitle.bind(viewTitle);
    }
}
