package org.appxi.smartlib.item;

import javafx.event.Event;
import javafx.event.EventType;

public class ItemEvent extends Event {
    private static final long serialVersionUID = 8183534581997098428L;

    public static final EventType<ItemEvent> VIEWING = new EventType<>(Event.ANY, "ITEM_VIEWING");
    public static final EventType<ItemEvent> VISITED = new EventType<>(Event.ANY, "ITEM_VISITED");

    public static final EventType<ItemEvent> EDITING = new EventType<>(Event.ANY, "ITEM_EDITING");
    //
    public static final EventType<ItemEvent> CREATED = new EventType<>(Event.ANY, "ITEM_CREATED");
    //
    public static final EventType<ItemEvent> RENAMED = new EventType<>(Event.ANY, "ITEM_RENAMED");
    //
    public static final EventType<ItemEvent> UPDATED = new EventType<>(Event.ANY, "ITEM_UPDATED");
    //
    public static final EventType<ItemEvent> DELETED = new EventType<>(Event.ANY, "ITEM_DELETED");
    //
    public static final EventType<ItemEvent> MOVED = new EventType<>(Event.ANY, "ITEM_MOVED");
    //
    public static final EventType<ItemEvent> RESTORED = new EventType<>(Event.ANY, "ITEM_RESTORED");
    //
    public final Item item, from;

    public ItemEvent(EventType<ItemEvent> eventType, Item item) {
        this(eventType, item, null);
    }

    public ItemEvent(EventType<ItemEvent> eventType, Item item, Item from) {
        super(eventType);
        this.item = item;
        this.from = from;
    }
}
