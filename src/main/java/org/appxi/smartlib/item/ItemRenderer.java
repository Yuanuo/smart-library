package org.appxi.smartlib.item;

import javafx.beans.binding.Bindings;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.javafx.workbench.views.WorkbenchMainViewController;

public abstract class ItemRenderer extends WorkbenchMainViewController {
    public final Item item;

    public ItemRenderer(Item item, WorkbenchPane workbench) {
        super(item.getPath(), workbench);
        this.item = item;
        //
        this.id.bind(item.path);
        this.title.bind(item.name);
        this.tooltip.bind(Bindings.createStringBinding(item::typedPath, item.path));
    }

    public abstract void navigate(Item item);
}
