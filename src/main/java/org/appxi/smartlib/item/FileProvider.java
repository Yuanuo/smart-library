package org.appxi.smartlib.item;

import javafx.scene.Node;
import javafx.scene.control.TreeItem;
import org.appxi.javafx.visual.MaterialIcon;

import java.util.function.Consumer;
import java.util.function.Function;

public final class FileProvider extends AbstractProvider {
    public static final FileProvider ONE = new FileProvider();

    private FileProvider() {
    }

    @Override
    public String providerId() {
        return null;
    }

    @Override
    public String providerName() {
        return "文件";
    }

    @Override
    public boolean isDirectory() {
        return false;
    }

    @Override
    public Node getItemIcon(TreeItem<Item> treeItem) {
        return MaterialIcon.BLOCK.graphic();
    }

    @Override
    public Consumer<Item> getCreator() {
        return null;
    }

    @Override
    public Function<Item, ItemEditor> getEditor() {
        return null;
    }

    @Override
    public Function<Item, ItemViewer> getViewer() {
        return null;
    }


//    private Consumer<Item> creator;
//
//    //    @Override
//    public Consumer<Item> getCreatorA() {
//        if (null != this.creator) return this.creator;
//        return this.creator = parent -> {
//            String str = DateHelper.format2(new Date()).replaceAll("[\s:]", "-");
//
//            Item item = new Item(this);
//            item.setName(str);
//            item.setPath(parent.isRoot() ? str : parent.getPath().concat("/").concat(str));
//            //
//            final String msg = DataApi.dataAccess().create(item);
//            if (null != msg) {
//                AppContext.toastError(msg);
//                return;
//            }
//            App.app().eventBus.fireEvent(new ItemEvent(ItemEvent.CREATED, item, parent));
//        };
//    }
}
