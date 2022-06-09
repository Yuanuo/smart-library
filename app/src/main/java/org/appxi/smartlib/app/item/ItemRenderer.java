package org.appxi.smartlib.app.item;

import org.appxi.javafx.workbench.WorkbenchPart;

public interface ItemRenderer extends WorkbenchPart.MainView {
    ItemEx item();

    void navigate(Object location);
}
