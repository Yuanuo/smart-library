package org.appxi.smartlib.app;

import javafx.beans.binding.Bindings;
import org.appxi.javafx.app.DesktopApp;
import org.appxi.javafx.app.web.WebViewer;
import org.appxi.javafx.workbench.WorkbenchPart;
import org.appxi.smartlib.app.item.ItemEx;

import java.nio.file.Path;
import java.util.List;
import java.util.stream.Stream;

public abstract class AppContext {

    private AppContext() {
    }

    public static List<String> getWebIncludeURIs() {
        List<String> result = WebViewer.getWebIncludeURIs();
        final Path dir = DesktopApp.appDir().resolve("template/web-incl");
        result.addAll(Stream.of("app.css", "app.js")
                .map(s -> dir.resolve(s).toUri().toString())
                .toList()
        );
        result.add(App.app().visualProvider.getWebStyleSheetLocationURI());
        return result;
    }

    public static void bindingViewer(WorkbenchPart.MainView viewer, ItemEx item) {
        viewer.id().bind(item.path);
        viewer.title().bind(item.name);
        viewer.tooltip().bind(Bindings.createStringBinding(item::toDetail, item.path));
        viewer.appTitle().bind(item.name);
    }

    public static void bindingEditor(WorkbenchPart.MainView editor, ItemEx item) {
        editor.id().bind(Bindings.createStringBinding(() -> "Edit@".concat(item.toDetail()), item.path));
        editor.title().bind(Bindings.createStringBinding(() -> "编辑: ".concat(item.getName()), item.name));
        editor.tooltip().bind(Bindings.createStringBinding(() -> "编辑: ".concat(item.toDetail()), item.path));
        editor.appTitle().bind(Bindings.createStringBinding(() -> "编辑: ".concat(item.getName()), item.path));
    }
}
