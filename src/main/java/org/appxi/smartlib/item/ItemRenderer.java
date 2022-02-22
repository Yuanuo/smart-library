package org.appxi.smartlib.item;

import javafx.beans.binding.Bindings;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.javafx.workbench.views.WorkbenchMainViewController;

public abstract class ItemRenderer extends WorkbenchMainViewController {
    public final Item item;
    protected final boolean editing;

    public ItemRenderer(Item item, WorkbenchPane workbench, boolean editing) {
        super(item.getPath(), workbench);
        this.item = item;
        this.editing = editing;
        //
        if (editing) {
            this.id.bind(Bindings.createStringBinding(() -> "Edit@".concat(item.typedPath()), item.path));
            this.title.bind(Bindings.createStringBinding(() -> "编辑: ".concat(item.getName()), item.name));
            this.tooltip.bind(Bindings.createStringBinding(() -> "编辑: ".concat(item.typedPath()), item.path));
        } else {
            this.id.bind(item.path);
            this.title.bind(item.name);
            this.tooltip.bind(Bindings.createStringBinding(item::typedPath, item.path));
        }
    }

    public abstract void navigate(Item item);
}
