package org.appxi.smartlib.app;

import javafx.scene.Scene;
import javafx.stage.Stage;
import org.appxi.file.FileWatcher;
import org.appxi.javafx.app.dict.DictionaryController;
import org.appxi.javafx.visual.VisualEvent;
import org.appxi.javafx.web.WebPane;
import org.appxi.javafx.workbench.WorkbenchApp;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.javafx.workbench.WorkbenchPart;
import org.appxi.prefs.UserPrefs;
import org.appxi.smartcn.pinyin.PinyinHelper;
import org.appxi.smartlib.app.explorer.LibraryExplorer;
import org.appxi.smartlib.app.home.AboutController;
import org.appxi.smartlib.app.home.PreferencesController;
import org.appxi.smartlib.app.recent.RecentItemsController;
import org.appxi.smartlib.app.recent.RecentViewsController;
import org.appxi.smartlib.app.search.LookupController;
import org.appxi.smartlib.app.search.SearchController;
import org.appxi.smartlib.dao.BeansContext;

import java.net.URL;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.Optional;

public class App extends WorkbenchApp {
    public static final String ID = "smartLibrary";
    public static final String NAME = "智悲研藏";
    public static final String VERSION = "22.06.19";
    private static App instance;

    public App() {
        App.instance = this;
    }

    public static App app() {
        return instance;
    }

    @Override
    public void init() throws Exception {
        super.init();
        //
        Locale.setDefault(Locale.SIMPLIFIED_CHINESE);
        new Thread(() -> {
            WebPane.preloadLibrary();
            BeansContext.setupInitialize(
                    App.app().eventBus,
                    UserPrefs.dataDir().resolve(".solr"),
                    App.appDir().resolve("template")
            );
        }).start();
        //
        PinyinHelper.pinyin("init");
    }

    @Override
    protected void showing(Stage primaryStage) {
        super.showing(primaryStage);
        if (productionMode) {
            Optional.ofNullable(App.class.getResource("app_desktop.css"))
                    .ifPresent(v -> primaryStage.getScene().getStylesheets().add(v.toExternalForm()));
        } else {
            Scene scene = primaryStage.getScene();
            visualProvider.visual().unAssign(scene);
            watchCss(scene, Path.of("../../appxi-javafx/src/main/resources/org/appxi/javafx/visual/visual_desktop.css"));
            watchCss(scene, Path.of("src/main/resources/org/appxi/smartlib/app/app_desktop.css"));
            scene.getStylesheets().forEach(System.out::println);
        }

        AppPreloader.hide();
    }

    private void watchCss(Scene scene, Path file) {
        try {
            final String filePath = file.toRealPath().toUri().toString().replace("///", "/");
            System.out.println("watch css: " + filePath);
            scene.getStylesheets().add(filePath);
        } catch (Exception e) {
            e.printStackTrace();
        }

        FileWatcher watcher = new FileWatcher(file.getParent());
        watcher.watching();
        watcher.addListener(event -> {
            if (event.type != FileWatcher.WatchType.MODIFY) return;
            if (event.getSource().getFileName().toString().endsWith("~")) return;
            String css = null;
            try {
                css = event.getSource().toRealPath().toUri().toString().replace("///", "/");
            } catch (Exception e) {
                e.printStackTrace();
            }
            if (null == css) return;
            System.out.println("CSS < " + css);
            if (css.endsWith("web.css")) {
                eventBus.fireEvent(new VisualEvent(VisualEvent.SET_STYLE, null));
            } else if (scene.getStylesheets().contains(css)) {
                final int idx = scene.getStylesheets().indexOf(css);
                String finalCss = css;
                javafx.application.Platform.runLater(() -> {
                    scene.getStylesheets().remove(finalCss);
                    if (idx != -1) scene.getStylesheets().add(idx, finalCss);
                    else scene.getStylesheets().add(finalCss);
                });
            }
        });
    }

    @Override
    public String getAppName() {
        return NAME;
    }

    @Override
    protected List<URL> getAppIcons() {
        final String[] iconSizes = new String[]{"32", "64", "128", "256"};
        final List<URL> result = new ArrayList<>(iconSizes.length);
        for (String iconSize : iconSizes) {
            result.add(App.class.getResource("icon-".concat(iconSize).concat(".png")));
        }
        return result;
    }

    @Override
    protected List<WorkbenchPart> createWorkbenchParts(WorkbenchPane workbench) {
        final List<WorkbenchPart> result = new ArrayList<>();

        result.add(new LibraryExplorer(workbench));

        result.add(new LookupController(workbench));
        result.add(new SearchController(workbench));
        result.add(new DictionaryController(workbench, AppContext::getWebIncludeURIs));

        result.add(new RecentViewsController(workbench));
        result.add(new RecentItemsController(workbench));

        result.add(new PreferencesController(workbench));
        result.add(new AboutController(workbench));
        return result;
    }
}
