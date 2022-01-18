package org.appxi.smartlib.item;

import javafx.beans.binding.Bindings;
import org.appxi.javafx.workbench.WorkbenchPane;

public abstract class ItemViewer extends ItemController {

    public ItemViewer(Item item, WorkbenchPane workbench) {
        super(item, workbench);
        //
        this.viewId.bind(Bindings.concat("View@", item.path));
        //
        this.viewTitle.bind(item.name);
        this.viewTooltip.bind(Bindings.createStringBinding(item::typedPath, item.path));
        this.appTitle.bind(item.name);
    }

    public abstract void navigate(Item item);
}
