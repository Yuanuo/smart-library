package org.appxi.smartlib.app;

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
import org.appxi.smartlib.app.item.ItemEx;
import org.appxi.smartlib.dao.ItemsDao;
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
import java.util.Objects;
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
        if (OSVersions.isLinux || OSVersions.isMac) {
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
        // ????????????????????????????????????????????????????????????????????????????????????????????????????????????
        if ("simple".equals(UserPrefs.prefsEx.getString("profile.mode", "simple"))) {
            final Path defaultDataDir = List.copyOf(profileMgr.getPropertyKeys()).stream()
                    .map(k -> new AbstractMap.SimpleEntry<>(k, profileMgr.getLong(k, -1)))
                    .sorted(Collections.reverseOrder(Comparator.comparingLong(AbstractMap.SimpleEntry::getValue)))
                    .map(val -> {
                        final String dir = val.getKey();
                        final Path path = Path.of(dir);
                        try {
                            if (!Files.isDirectory(path) || FileHelper.notExists(path)
                                || FileHelper.exists(path) && FileHelper.notExists(path.resolve(dataDirName))) {
                                return null;
                            }
                        } catch (Throwable ignore) {
                        }
                        return path;
                    })
                    .filter(Objects::nonNull)
                    .findFirst()
                    .orElse(newDataDir);
            if (tryLoadProfile(profileMgr, defaultDataDir)) return;
        }
        //
        while (true) {
            final CardChooser cardChooser = CardChooser.of("?????????????????? - ".concat(App.NAME))
                    .header("???????????????????????????????????????????????????????????????????????????????????????????????????", null)
                    .owner(primaryStage);
            cardChooser.cards(CardChooser.ofCard("???????????????")
                    .description("?????????????????????????????????????????????????????????????????????????????????????????????")
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

            cardChooser.cards(CardChooser.ofCard("??????")
                    .description("?????????")
                    .graphic(MaterialIcon.EXIT_TO_APP.graphic())
                    .get());

            final Optional<CardChooser.Card> optional = cardChooser.showAndWait();
            if (optional.isEmpty() || optional.get().userData() == null) {
                System.exit(0);
                return;
            }
            Path selectedPath = null;
            // ??????????????????
            if (optional.get().userData() == Boolean.TRUE) {
                final DirectoryChooser chooser = new DirectoryChooser();
                chooser.setTitle("???????????????");
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
            // ?????????????????????????????????????????????????????????????????????
            ItemsDao.setupInitialize(basePath, ItemEx.ROOT);
            //
            return true;
        } catch (Throwable e) {
            Alert alert = new Alert(Alert.AlertType.ERROR, basePath.toString());
            alert.setHeaderText("????????????????????????????????????????????????????????????????????????");
            alert.initOwner(primaryStage);
            alert.showAndWait();
            return false;
        }
    }
}
