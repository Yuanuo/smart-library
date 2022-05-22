package org.appxi.smartlib;

import javafx.application.Preloader;
import javafx.scene.Scene;
import javafx.scene.control.Alert;
import javafx.scene.image.Image;
import javafx.scene.layout.BorderPane;
import javafx.stage.DirectoryChooser;
import javafx.stage.Stage;
import javafx.stage.StageStyle;
import org.appxi.javafx.control.CardChooser;
import org.appxi.javafx.helper.FontFaceHelper;
import org.appxi.javafx.visual.MaterialIcon;
import org.appxi.javafx.visual.VisualProvider;
import org.appxi.prefs.Preferences;
import org.appxi.prefs.PreferencesInProperties;
import org.appxi.prefs.UserPrefs;
import org.appxi.smartlib.dao.DataApi;
import org.appxi.util.FileHelper;
import org.appxi.util.OSVersions;

import java.io.File;
import java.io.FileOutputStream;
import java.nio.channels.FileLock;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.AbstractMap;
import java.util.Collections;
import java.util.Comparator;
import java.util.List;
import java.util.Optional;

public class AppPreloader extends Preloader {
    private static Stage primaryStage;
    static FileLock profileLocker;
    static String dataDirName;

    @Override
    public void start(Stage primaryStage) throws Exception {
        AppPreloader.primaryStage = primaryStage;

        primaryStage.initStyle(StageStyle.TRANSPARENT);
        primaryStage.setOpacity(0);
        primaryStage.setScene(new Scene(new BorderPane(), 800, 600));

        primaryStage.setTitle(App.NAME);
        Optional.ofNullable(getClass().getResourceAsStream("icon-32.png"))
                .ifPresent(v -> primaryStage.getIcons().setAll(new Image(v)));
        primaryStage.centerOnScreen();
        primaryStage.show();
        //
        new VisualProvider(null, primaryStage::getScene).initialize();
        Optional.ofNullable(AppPreloader.class.getResource("app_desktop.css"))
                .ifPresent(v -> primaryStage.getScene().getStylesheets().add(v.toExternalForm()));
        //
        setupChooser(primaryStage);
        //
        FontFaceHelper.fixing();
        //
        if (OSVersions.isLinux) {
            new javafx.scene.control.TextField("");
            new javax.swing.JTextField("");
        }
    }

    public static void hide() {
        if (null != primaryStage) primaryStage.close();
    }

    static void setupChooser(Stage primaryStage) {
        dataDirName = ".".concat(App.ID);
        UserPrefs.prefsEx = new PreferencesInProperties(UserPrefs.dataDir().resolve(".prefs"));
        final Preferences profileMgr = new PreferencesInProperties(UserPrefs.dataDir().resolve(".profile"));

        String dataDirStr = UserPrefs.dataDir().toString();
        dataDirStr = dataDirStr.substring(0, dataDirStr.length() - 2);
        final Path oldDataDir = Path.of(dataDirStr);
        final Path newDataDir = Path.of(dataDirStr.concat(".dd"));
        if (FileHelper.exists(oldDataDir)) {
            try {
                Files.move(oldDataDir.resolve(".db"), newDataDir);
                Files.move(oldDataDir, newDataDir.resolve(oldDataDir.getFileName()));
            } catch (Throwable e) {
                e.printStackTrace();
            }
        }

        if ("simple".equals(UserPrefs.prefsEx.getString("profile.mode", "simple"))) {
            if (tryLoadProfile(profileMgr, newDataDir)) return;
        }
        //
        while (true) {
            final CardChooser cardChooser = CardChooser.of("选择项目位置 - ".concat(App.NAME))
                    .header("使用项目管理数据，适用于多开、数据隔离、提升性能和效率的使用方式！", null)
                    .owner(primaryStage);
            cardChooser.cards(CardChooser.ofCard("打开文件夹")
                    .description("选择文件夹作为项目，必须具备写入权限和可用容量以存放所有数据！")
                    .graphic(MaterialIcon.FOLDER.graphic())
                    .userData(Boolean.TRUE)
                    .get());
            List.copyOf(profileMgr.getPropertyKeys()).stream()
                    .map(k -> new AbstractMap.SimpleEntry<>(k, profileMgr.getLong(k, -1)))
                    .sorted(Collections.reverseOrder(Comparator.comparingLong(AbstractMap.SimpleEntry::getValue)))
                    .forEach(val -> {
                        try {
                            final String dir = val.getKey();
                            final Path path = Path.of(dir);
                            if (!Files.isDirectory(path) || FileHelper.notExists(path)
                                    || FileHelper.exists(path) && FileHelper.notExists(path.resolve(dataDirName))) {
                                profileMgr.removeProperty(dir);
                                return;
                            }

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
            Path selectedPath = null;
            // 选择数据目录
            if (optional.get().userData() == Boolean.TRUE) {
                final DirectoryChooser chooser = new DirectoryChooser();
                chooser.setTitle("打开文件夹");
                final File selected = chooser.showDialog(primaryStage);
                if (null == selected) continue;
                selectedPath = selected.toPath();
            } else if (optional.get().userData() instanceof String str) {
                selectedPath = Path.of(str);
            } else if (optional.get().userData() instanceof Path path) {
                selectedPath = path;
            }
            if (null == selectedPath) continue;
            if (tryLoadProfile(profileMgr, selectedPath)) break;
        }
    }

    static boolean tryLoadProfile(Preferences profileMgr, Path basePath) {
        try {
            final Path lockFile = basePath.resolve(dataDirName).resolve(".lock");
            FileHelper.makeParents(lockFile);
            FileHelper.setHidden(lockFile.getParent(), true);

            profileLocker = new FileOutputStream(lockFile.toFile()).getChannel().tryLock();
            if (null == profileLocker) throw new IllegalAccessException("cannot lock");
            //
            profileMgr.setProperty(basePath.toString(), System.currentTimeMillis());
            profileMgr.save();

            UserPrefs.setupDataDirectory(lockFile.getParent(), null);
            UserPrefs.prefs = new PreferencesInProperties(UserPrefs.confDir().resolve(".prefs"));
            // 在此设置数据库基本环境，以供后续的功能正常使用
            DataApi.setupInitialize(basePath);
            return true;
        } catch (Throwable e) {
            Alert alert = new Alert(Alert.AlertType.ERROR, basePath.toString());
            alert.setHeaderText("无法锁定项目位置，可能正在使用！请尝试其他选项。");
            alert.initOwner(primaryStage);
            alert.showAndWait();
            return false;
        }
    }
}
