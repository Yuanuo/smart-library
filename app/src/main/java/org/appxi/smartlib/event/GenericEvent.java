package org.appxi.smartlib.event;

import org.appxi.event.Event;
import org.appxi.event.EventType;

public class GenericEvent extends Event {
    private static final long serialVersionUID = 1L;

    public static final EventType<GenericEvent> BEANS_READY = new EventType<>(Event.ANY, "BEANS_READY");

    public static final EventType<GenericEvent> DISPLAY_HAN_CHANGED = new EventType<>(Event.ANY, "DISPLAY_HAN_CHANGED");

    public final Object data;

    public GenericEvent(EventType<GenericEvent> eventType) {
        this(eventType, null);
    }

    public GenericEvent(EventType<GenericEvent> eventType, Object data) {
        super(eventType);
        this.data = data;
    }
}
