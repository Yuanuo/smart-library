module appxi.smartLibrary {
    requires javafx.base;
    requires javafx.controls;
    requires javafx.graphics;
    requires javafx.web;
    requires javafx.swing;
    requires jdk.jsobject;

    requires appxi.shared;
    requires appxi.javafx;
    requires appxi.timeago;
    requires appxi.smartcn.pinyin;
    requires org.jsoup;
    requires org.json;

    requires java.sql;

    requires appxi.search.solr;

//    requires org.scenicview.scenicview; // for debug

    exports org.appxi.smartlib; // for application launch
    exports org.appxi.smartlib.html;
    exports org.appxi.smartlib.item;
    exports org.appxi.smartlib.item.mindmap;

    opens org.appxi.smartlib;
    opens org.appxi.smartlib.dao;
}