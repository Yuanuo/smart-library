package org.appxi.smartlib.explorer;

import javafx.scene.control.Tooltip;
import javafx.scene.control.TreeCell;
import javafx.scene.control.TreeItem;
import javafx.scene.control.TreeView;
import javafx.scene.input.ClipboardContent;
import javafx.scene.input.DataFormat;
import javafx.scene.input.DragEvent;
import javafx.scene.input.Dragboard;
import javafx.scene.input.MouseEvent;
import javafx.scene.input.TransferMode;
import javafx.util.Callback;
import org.appxi.prefs.UserPrefs;
import org.appxi.smartlib.item.Item;

import java.util.Objects;

class LibraryTreeCell implements Callback<TreeView<Item>, TreeCell<Item>> {
    private static final DataFormat PLAIN_ITEM = new DataFormat("application/x-item-serialized-object");
    private static final String DROP_HINT_STYLE = "-fx-border-color: #eea82f; -fx-border-width: 2; -fx-padding: 1;";
    private TreeCell<Item> dropZone;
    private TreeItem<Item> draggedItem;

    @Override
    public TreeCell<Item> call(TreeView<Item> treeView) {
        final TreeCell<Item> cell = new TreeCell<>() {
            Item updatedItem;

            @Override
            protected void updateItem(Item item, boolean empty) {
                super.updateItem(item, empty);
                if (empty) {
                    updatedItem = null;
                    this.textProperty().unbind();
                    this.setText(null);
                    this.setGraphic(null);
                    this.setTooltip(null);
                    return;
                }
                if (item == updatedItem)
                    return;//
                updatedItem = item;
                this.textProperty().bind(item.name);
                this.setGraphic(item.provider.getItemIcon(getTreeItem()));
                this.setTooltip(new Tooltip(item.getName()));
                //
                this.getStyleClass().remove("visited");
                if (UserPrefs.recents.containsProperty(item.getPath())) {
                    this.getStyleClass().add("visited");
                }
            }
        };
        cell.setOnDragDetected((MouseEvent event) -> dragDetected(event, cell));
        cell.setOnDragOver((DragEvent event) -> dragOver(event, cell));
        cell.setOnDragDropped((DragEvent event) -> dragDropped(event, cell));
        cell.setOnDragDone((DragEvent event) -> clearDropLocation());
        cell.setOnDragExited((DragEvent event) -> clearDropLocation());

        return cell;
    }

    private void dragDetected(MouseEvent event, TreeCell<Item> treeCell) {
        draggedItem = treeCell.getTreeItem();

        // root can't be dragged
        if (null == draggedItem || draggedItem.getParent() == null) return;
        Dragboard db = treeCell.startDragAndDrop(TransferMode.MOVE);

        ClipboardContent content = new ClipboardContent();
        content.put(PLAIN_ITEM, draggedItem.getValue().getPath());
        db.setContent(content);
        db.setDragView(treeCell.snapshot(null, null));
        event.consume();
    }

    private void dragOver(DragEvent event, TreeCell<Item> treeCell) {
        if (!event.getDragboard().hasContent(PLAIN_ITEM)) return;

        final TreeItem<Item> thisItem = treeCell.getTreeItem();
        // can't drop on itself
        if (draggedItem == null || thisItem == null || thisItem == draggedItem)
            return;
        // can't drop on same folder
        if (draggedItem.getParent() == thisItem.getParent() && thisItem.isLeaf())
            return;
        // can't drop on sub folder
        if (thisItem.getValue().getPath().startsWith(draggedItem.getValue().getPath()))
            return;
        // ignore if this is the root
        if (draggedItem.getParent() == null) {
            clearDropLocation();
            return;
        }

        event.acceptTransferModes(TransferMode.MOVE);
        if (!Objects.equals(dropZone, treeCell)) {
            clearDropLocation();
            this.dropZone = treeCell;
            dropZone.setStyle(DROP_HINT_STYLE);
        }
    }

    private void dragDropped(DragEvent event, TreeCell<Item> treeCell) {
        if (!event.getDragboard().hasContent(PLAIN_ITEM)) return;

        final TreeItem<Item> newItem = treeCell.getTreeItem();
        final TreeItem<Item> newParent = newItem.getValue().provider.isDirectory() ? newItem : newItem.getParent();
        try {
            ItemActions.move(draggedItem.getValue(),
                    (newItem == draggedItem.getParent() ? newParent.getParent() : newParent).getValue());
        } finally {
            event.setDropCompleted(true);
        }
    }

    private void clearDropLocation() {
        if (dropZone != null) dropZone.setStyle("");
        dropZone = null;
    }
}