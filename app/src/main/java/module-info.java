module appxi.smartLibrary {
    requires javafx.swing;

    requires appxi.javafx;
    requires appxi.timeago;

    requires java.sql;

    requires appxi.smartLibrary.api;

//    requires org.scenicview.scenicview; // for debug

    exports org.appxi.smartlib.app; // for application launch
    exports org.appxi.smartlib.app.item; // for js-engine
    exports org.appxi.smartlib.app.item.mindmap; // for js-engine


    opens org.appxi.smartlib.app;
}