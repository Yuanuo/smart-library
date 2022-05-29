package org.appxi.smartlib.app.explorer;

import javafx.collections.ListChangeListener;
import javafx.collections.ObservableList;
import javafx.scene.control.TreeItem;
import org.appxi.smartlib.FolderProvider;
import org.appxi.smartlib.Item;

import java.util.ArrayList;
import java.util.List;
import java.util.Objects;

class LibraryTreeItem extends TreeItem<Item> {
    private boolean isFirstTimeChildren = true;
    private boolean isFirstTimeLeaf = true;
    private boolean isLeaf;

    private final ListChangeListener<TreeItem<Item>> childrenListener = change -> {
        if (change.next() && change.wasAdded() && change.getAddedSize() > 0) {
            change.getList().sort(ItemActions.sortByName);
        }
    };

    public LibraryTreeItem(Item value) {
        super(value);
    }

    @Override
    public ObservableList<TreeItem<Item>> getChildren() {
        if (isFirstTimeChildren) {
            isFirstTimeChildren = false;

            List<Item> items = (this.getValue().provider instanceof FolderProvider folderProvider)
                    ? folderProvider.getChildren(this.getValue())
                    : new ArrayList<>(0);
            if (null == items)
                items = new ArrayList<>(0);

            final ObservableList<TreeItem<Item>> children = super.getChildren();
            children.addListener(childrenListener);
            children.setAll(items.stream().map(LibraryTreeItem::new).toList());
            return children;
        }
        return super.getChildren();
    }

    @Override
    public boolean isLeaf() {
        if (isFirstTimeLeaf) {
            isFirstTimeLeaf = false;
            isLeaf = !this.getValue().provider.isFolder();
        }
        return isLeaf;
    }

    public boolean isLoaded() {
        return !isFirstTimeChildren;
    }

    public LibraryTreeItem findTreeItem(String path) {
        TreeItem<Item> curr = this, prev;
        do {
            prev = curr;
            for (TreeItem<Item> next : curr.getChildren()) {
                if (path.equals(next.getValue().getPath())) {
                    return (LibraryTreeItem) next;
                }
                if (path.startsWith(next.getValue().getPath().concat("/"))) {
                    curr = next;
                    break;
                }
            }
        } while (curr != prev);
        return null;
    }

    public void reset() {
        isFirstTimeLeaf = true;
        isFirstTimeChildren = true;
        this.setExpanded(false);
        super.getChildren().removeListener(childrenListener);
    }

    public void rebase(String oldBase, String newBase) {
        if (!Objects.equals(getValue().getPath(), newBase))
            getValue().setPath(newBase.concat(getValue().getPath().substring(oldBase.length())));
        super.getChildren().forEach(itm -> ((LibraryTreeItem) itm).rebase(oldBase, newBase));
    }

    public void resort() {
        getParent().getChildren().sort(ItemActions.sortByName);
    }

    @Override
    public boolean equals(Object other) {
        if (null == other) return false;
        if (this == other) return true;
        if (other instanceof LibraryTreeItem node)
            return Objects.equals(this.getValue(), node.getValue());
        return false;
    }
}
