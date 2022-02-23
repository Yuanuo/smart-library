package org.appxi.smartlib;

import javafx.application.Preloader;
import javafx.scene.Scene;
import javafx.scene.control.Alert;
import javafx.scene.image.Image;
import javafx.scene.image.ImageView;
import javafx.scene.layout.BorderPane;
import javafx.stage.DirectoryChooser;
import javafx.stage.Stage;
import javafx.stage.StageStyle;
import org.appxi.javafx.control.CardChooser;
import org.appxi.javafx.visual.MaterialIcon;
import org.appxi.javafx.visual.Swatch;
import org.appxi.javafx.visual.Theme;
import org.appxi.javafx.visual.Visual;
import org.appxi.prefs.Preferences;
import org.appxi.prefs.PreferencesInProperties;
import org.appxi.prefs.UserPrefs;
import org.appxi.smartlib.dao.DataApi;
import org.appxi.util.FileHelper;

import java.io.File;
import java.io.FileOutputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.AbstractMap;
import java.util.Collections;
import java.util.Comparator;
import java.util.Optional;

public class AppPreloader extends Preloader {
    private static Stage primaryStage;
    static FileOutputStream profileLocker;

    @Override
    public void start(Stage primaryStage) throws Exception {
        AppPreloader.primaryStage = primaryStage;

        final ImageView imageView = new ImageView();
        Optional.ofNullable(AppPreloader.class.getResourceAsStream("splash.jpg"))
                .ifPresent(v -> imageView.setImage(new Image(v)));

        primaryStage.initStyle(StageStyle.TRANSPARENT);
        primaryStage.setScene(new Scene(new BorderPane(imageView), 800, 533));

        Optional.ofNullable(getClass().getResourceAsStream("icon-32.png"))
                .ifPresent(v -> primaryStage.getIcons().setAll(new Image(v)));
        primaryStage.centerOnScreen();
        primaryStage.show();
        setupChooser(primaryStage);
        new javafx.scene.control.TextField("");
        new javax.swing.JTextField("");
    }

    public static void hide() {
        if (null != primaryStage) primaryStage.close();
    }

    static void setupChooser(Stage primaryStage) {
        final Preferences profileMgr = new PreferencesInProperties(UserPrefs.dataDir().resolve(".profile"));
        //
        primaryStage.getScene().getRoot().setStyle("-fx-font-size: 16px;");
        Theme.getDefault().assignTo(primaryStage.getScene());
        Swatch.getDefault().assignTo(primaryStage.getScene());
        Visual.getDefault().assignTo(primaryStage.getScene());
        Optional.ofNullable(AppPreloader.class.getResource("app_desktop.css"))
                .ifPresent(v -> primaryStage.getScene().getStylesheets().add(v.toExternalForm()));
        //
        while (true) {
            final CardChooser cardChooser = CardChooser.of("选择数据空间 - ".concat(App.NAME))
                    .header("数据空间，适用于多开、数据隔离、提升性能和效率的使用方式！", null)
                    .owner(primaryStage);
            cardChooser.cards(CardChooser.ofCard("打开目录")
                    .description("作为数据空间的目录用于存放数据，必须具有写入权限和可用容量！")
                    .graphic(MaterialIcon.FOLDER.graphic())
                    .userData(Boolean.TRUE)
                    .get());
            profileMgr.getPropertyKeys().stream()
                    .map(k -> new AbstractMap.SimpleEntry<>(k, profileMgr.getLong(k, -1)))
                    .sorted(Collections.reverseOrder(Comparator.comparingLong(AbstractMap.SimpleEntry::getValue)))
                    .forEach(val -> {
                        try {
                            final String dir = val.getKey();
                            final Path path = Path.of(dir);
                            if (!Files.isDirectory(path)) return;

                            cardChooser.cards(CardChooser.ofCard(path.getFileName().toString())
                                    .description(path.toString())
                                    .graphic(MaterialIcon.FOLDER_OPEN.graphic())
                                    .userData(dir)
                                    .get());
                        } catch (Throwable ignore) {
                        }
                    });

            cardChooser.cards(CardChooser.ofCard("退出")
                    .description("请退出")
                    .graphic(MaterialIcon.EXIT_TO_APP.graphic())
                    .get());

            final Optional<CardChooser.Card> optional = cardChooser.showAndWait();
            if (optional.isEmpty() || optional.get().userData() == null) {
                System.exit(0);
                return;
            }
            String selectedDir = null;
            // 选择数据目录
            if (optional.get().userData() == Boolean.TRUE) {
                final DirectoryChooser chooser = new DirectoryChooser();
                chooser.setTitle("打开目录");
                final File selected = chooser.showDialog(primaryStage);
                if (null == selected) continue;
                selectedDir = selected.getAbsolutePath();
            } else if (optional.get().userData() instanceof String str) {
                selectedDir = str;
            }
            if (null == selectedDir) continue;
            //
            final Path basePath = Path.of(selectedDir);
            final Path lockFile = basePath.resolve(".".concat(App.ID)).resolve(".lock");
            FileHelper.makeParents(lockFile);

            try {
                profileLocker = new FileOutputStream(lockFile.toFile());
                profileLocker.getChannel().lock();
            } catch (Throwable e) {
                Alert alert = new Alert(Alert.AlertType.ERROR, "无法锁定目录，可能正在使用！请尝试其他选项。");
                alert.initOwner(primaryStage);
                alert.show();
                continue;
            }
            //
            profileMgr.setProperty(selectedDir, System.currentTimeMillis());
            profileMgr.save();

            UserPrefs.setupDataDirectory(lockFile.getParent(), null);
            UserPrefs.prefs = new PreferencesInProperties(UserPrefs.confDir().resolve(".prefs"));
            // 在此设置数据库基本环境，以供后续的功能正常使用
            DataApi.setupInitialize(basePath);
            break;
        }
    }
}
