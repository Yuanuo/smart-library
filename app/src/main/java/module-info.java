module appxi.smartLibrary {
    requires javafx.base;
    requires javafx.controls;
    requires javafx.graphics;
    requires javafx.web;
    requires javafx.swing;
    requires jdk.jsobject;

    requires appxi.javafx;
    requires appxi.timeago;
    requires appxi.smartcn.convert;

    requires java.sql;

    requires appxi.dictionary;
    requires appxi.smartLibrary.api;

//    requires org.scenicview.scenicview; // for debug

    exports org.appxi.smartlib.app; // for application launch
    exports org.appxi.smartlib.app.html;
    exports org.appxi.smartlib.app.item;
    exports org.appxi.smartlib.app.item.mindmap;
    exports org.appxi.smartlib.app.dict;


    opens org.appxi.smartlib.app;
}