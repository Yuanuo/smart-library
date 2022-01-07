module appxi.smartLibrary {
    requires java.logging;
    requires javafx.base;
    requires javafx.controls;
    requires javafx.graphics;
    requires javafx.web;
    requires javafx.swing;
    requires jdk.jsobject;
    requires java.desktop;

    requires appxi.shared;
    requires appxi.javafx;
    requires appxi.timeago;
    requires appxi.smartcn.pinyin;
    requires org.jsoup;

    requires java.sql;

    requires appxi.search.solr;
    requires static appxi.search.solr.aio;
    requires static appxi.search.tika.aio;
    requires static spring.core;
    requires static spring.context;
    requires static spring.beans;
    requires static spring.data.commons;
    requires static spring.data.solr;
    requires static spring.tx;
    requires static org.apache.logging.log4j.core;
    requires static org.apache.logging.log4j.slf4j;
    requires static org.apache.logging.log4j.web;
    requires org.apache.logging.log4j;
    requires org.slf4j;
    requires org.slf4j.simple;
    requires org.apache.commons.logging;
    requires com.fasterxml.jackson.annotation;
    requires com.fasterxml.jackson.core;
    requires com.fasterxml.jackson.databind;
    requires java.management;
    requires java.naming;

//    requires org.scenicview.scenicview; // for debug

    exports org.appxi.smartlib; // for application launch
    exports org.appxi.smartlib.html;

    opens org.appxi.smartlib;
    opens org.appxi.smartlib.dao;
}