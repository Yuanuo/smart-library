package org.appxi.smartlib.home;

import javafx.beans.property.ObjectProperty;
import javafx.beans.property.SimpleObjectProperty;
import javafx.scene.Node;
import javafx.scene.control.ButtonType;
import javafx.scene.control.Dialog;
import javafx.scene.control.DialogPane;
import org.appxi.javafx.settings.DefaultOption;
import org.appxi.javafx.settings.SettingsPane;
import org.appxi.javafx.visual.MaterialIcon;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.javafx.workbench.views.WorkbenchSideToolController;
import org.appxi.prefs.UserPrefs;

public class PreferencesController extends WorkbenchSideToolController {
    public PreferencesController(WorkbenchPane workbench) {
        super("PREFERENCES", workbench);
        this.setTitles("设置");
        this.graphic.set(MaterialIcon.TUNE.graphic());
    }

    @Override
    public void initialize() {
    }

    @Override
    public void onViewportShowing(boolean firstTime) {
        SettingsPane settingsPane = new SettingsPane();

        final ObjectProperty<ProfileMode> profileModeProperty = new SimpleObjectProperty<>();
        profileModeProperty.setValue(ProfileMode.of(UserPrefs.prefsEx.getString("profile.mode", "simple")));
        profileModeProperty.addListener((o, ov, nv) -> UserPrefs.prefsEx.setProperty("profile.mode", nv.name()).save());
        settingsPane.getOptions().add(new DefaultOption<ProfileMode>("项目位置", "切换项目位置选择方式", "PROJECT", true)
                .setValueProperty(profileModeProperty));

        settingsPane.getOptions().add(app.visualProvider.optionForFontSmooth());
        settingsPane.getOptions().add(app.visualProvider.optionForFontName());
        settingsPane.getOptions().add(app.visualProvider.optionForFontSize());
        settingsPane.getOptions().add(app.visualProvider.optionForTheme());
        settingsPane.getOptions().add(app.visualProvider.optionForSwatch());
        settingsPane.getOptions().add(app.visualProvider.optionForWebFontName());
        settingsPane.getOptions().add(app.visualProvider.optionForWebFontSize());
        settingsPane.getOptions().add(app.visualProvider.optionForWebPageColor());
        settingsPane.getOptions().add(app.visualProvider.optionForWebTextColor());

        final ObjectProperty<EnterAction> enterActionProperty = new SimpleObjectProperty<>();
        enterActionProperty.setValue(EnterAction.of(UserPrefs.prefs.getString("explorer.enterAction", "view")));
        enterActionProperty.addListener((o, ov, nv) -> UserPrefs.prefs.setProperty("explorer.enterAction", nv.name()));
        settingsPane.getOptions().add(new DefaultOption<EnterAction>("打开方式", "资源管理器默认双击动作", "EXPLORER", true)
                .setValueProperty(enterActionProperty));

        final DialogPane dialogPane = new DialogPane() {
            @Override
            protected Node createButtonBar() {
                return null;
            }
        };
//        dialogPane.setPrefSize(480, 640);
        dialogPane.setContent(settingsPane);
        dialogPane.getButtonTypes().add(ButtonType.OK);
        //
        Dialog<?> dialog = new Dialog<>();
        dialog.setTitle(title.get());
        dialog.setDialogPane(dialogPane);
        dialog.getDialogPane().setPrefWidth(600);
        dialog.initOwner(app.getPrimaryStage());
        dialog.show();
    }

    private enum EnterAction {
        view("查看"),
        edit("编辑"),
        none("无操作");
        final String title;

        EnterAction(String title) {
            this.title = title;
        }

        @Override
        public String toString() {
            return title;
        }

        static EnterAction of(String name) {
            try {
                return valueOf(name);
            } catch (Exception ignore) {
            }
            return view;
        }
    }

    private enum ProfileMode {
        simple("默认项目位置"),
        advanced("启动时选择项目位置");
        final String title;

        ProfileMode(String title) {
            this.title = title;
        }

        @Override
        public String toString() {
            return title;
        }

        static ProfileMode of(String name) {
            try {
                return valueOf(name);
            } catch (Exception ignore) {
            }
            return simple;
        }
    }
}
