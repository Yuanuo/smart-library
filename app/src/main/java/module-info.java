module appxi.smartLibrary {
    requires javafx.swing;

    requires appxi.javafx;
    requires appxi.timeago;

    requires java.sql;

    requires appxi.smartLibrary.api;
    requires appxi.smartDictionary;

    exports org.appxi.smartlib.app; // for application launch
    exports org.appxi.smartlib.app.item;
    exports org.appxi.smartlib.app.item.article;
    exports org.appxi.smartlib.app.item.mindmap;
    exports org.appxi.smartlib.app.item.tika;


    opens org.appxi.smartlib.app;
}