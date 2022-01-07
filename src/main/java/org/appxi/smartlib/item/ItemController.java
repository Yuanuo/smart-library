package org.appxi.smartlib.item;

import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.javafx.workbench.views.WorkbenchMainViewController;

public abstract class ItemController extends WorkbenchMainViewController {
    public final Item item;

    public ItemController(Item item, WorkbenchPane workbench) {
        super(item.getPath(), workbench);
        this.item = item;
    }
}
