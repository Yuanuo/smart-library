package org.appxi.smartlib.app.item;

import javafx.beans.binding.Bindings;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.javafx.workbench.views.WorkbenchMainViewController;
import org.appxi.smartlib.Item;

public abstract class ItemRenderer extends WorkbenchMainViewController {
    public final Item item;
    protected final boolean editing;

    public ItemRenderer(Item item, WorkbenchPane workbench, boolean editing) {
        super(item.getPath(), workbench);
        this.item = item;
        this.editing = editing;
        //
        if (editing) {
            this.id.bind(Bindings.createStringBinding(() -> "Edit@".concat(item.toDetail())));
            this.title.bind(Bindings.createStringBinding(() -> "编辑: ".concat(item.getName())));
            this.tooltip.bind(Bindings.createStringBinding(() -> "编辑: ".concat(item.toDetail())));
        } else {
            this.id.bind(Bindings.createStringBinding(item::getPath));
            this.title.bind(Bindings.createStringBinding(item::getName));
            this.tooltip.bind(Bindings.createStringBinding(item::toDetail));
        }
    }

    public abstract void navigate(Item item);
}
