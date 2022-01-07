package org.appxi.smartlib.item;

import javafx.scene.control.TreeItem;
import org.appxi.smartlib.AppContext;

import java.util.Comparator;

public abstract class ItemHelper {
    public static Comparator<? super TreeItem<Item>> sortByName
            = Comparator.comparing(v -> (v.getValue().provider.isDirectory() ? "0" : "1")
            .concat(AppContext.ascii(v.getValue().getName())));

    public static Comparator<? super TreeItem<Item>> sortByPath
            = Comparator.comparing(v -> (v.getValue().provider.isDirectory() ? "0" : "1").concat(v.getValue().getPath()));

    private ItemHelper() {
    }

    public static boolean isNameWithProvider(String name, String providerId) {
        return null != providerId && !providerId.isBlank() && name.toLowerCase().endsWith(".".concat(providerId));
    }

    public static String nameWithProvider(String name, String providerId) {
        if (null == providerId || providerId.isBlank()) return name;
        final String ext = ".".concat(providerId);
        return name.endsWith(ext) ? name : name.concat(ext);
    }

    public static String nameWithoutProvider(String name, String providerId) {
        return null == providerId || providerId.isBlank()
                ? name : name.substring(0, name.length() - providerId.length() - 1);
    }
}
