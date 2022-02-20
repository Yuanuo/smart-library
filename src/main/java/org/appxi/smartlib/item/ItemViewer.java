package org.appxi.smartlib.item;

import javafx.beans.binding.Bindings;
import org.appxi.javafx.workbench.WorkbenchPane;

public abstract class ItemViewer extends ItemController {

    public ItemViewer(Item item, WorkbenchPane workbench) {
        super(item, workbench);
        //
        this.id.bind(Bindings.createStringBinding(() -> "View@".concat(item.typedPath()), item.path));
        //
        this.title.bind(item.name);
        this.tooltip.bind(Bindings.createStringBinding(item::typedPath, item.path));
    }

    public abstract void navigate(Item item);
}
