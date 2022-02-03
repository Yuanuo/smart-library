package org.appxi.smartlib.home;

import javafx.beans.property.Property;
import javafx.beans.property.SimpleStringProperty;
import javafx.beans.property.StringProperty;
import javafx.scene.Node;
import javafx.scene.control.ButtonType;
import javafx.scene.control.ChoiceBox;
import javafx.scene.control.Dialog;
import javafx.scene.control.DialogPane;
import org.appxi.javafx.settings.DefaultOption;
import org.appxi.javafx.settings.OptionEditorBase;
import org.appxi.javafx.settings.SettingsPane;
import org.appxi.javafx.visual.MaterialIcon;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.javafx.workbench.views.WorkbenchSideToolController;
import org.appxi.prefs.UserPrefs;

import java.util.Objects;

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

        settingsPane.getOptions().add(app.visualProvider.optionForFontSmooth());
        settingsPane.getOptions().add(app.visualProvider.optionForFontName());
        settingsPane.getOptions().add(app.visualProvider.optionForFontSize());
        settingsPane.getOptions().add(app.visualProvider.optionForTheme());
        settingsPane.getOptions().add(app.visualProvider.optionForSwatch());
        settingsPane.getOptions().add(app.visualProvider.optionForWebFontName());
        settingsPane.getOptions().add(app.visualProvider.optionForWebFontSize());

        settingsPane.getOptions().add(new DefaultOption<>(
                "打开方式", "资源管理器默认双击动作", "EXPLORER",
                UserPrefs.prefs.getString("explorer.enterAction", "view"), true,
                option -> new OptionEditorBase<String, ChoiceBox<String>>(option, new ChoiceBox<>()) {
                    private StringProperty valueProperty;

                    @Override
                    public Property<String> valueProperty() {
                        if (this.valueProperty == null) {
                            this.valueProperty = new SimpleStringProperty();
                            this.getEditor().getItems().setAll(
                                    "view:查看", "edit:编辑", "none:无"
                            );
                            this.getEditor().getSelectionModel().selectedItemProperty().addListener((obs, ov, nv) -> {
                                if (ov == null || Objects.equals(ov, nv)) return;
                                this.valueProperty.set(nv);
                                //
                                UserPrefs.prefs.setProperty("explorer.enterAction", nv.split(":", 2)[0]);
                            });
                            this.valueProperty.addListener((obs, ov, nv) -> this.setValue(nv));
                        }
                        return this.valueProperty;
                    }

                    @Override
                    public void setValue(String value) {
                        if (getEditor().getItems().isEmpty()) return;
                        getEditor().getItems().stream().filter(v -> v.startsWith(value))
                                .findFirst()
                                .ifPresentOrElse(v -> getEditor().setValue(v),
                                        () -> getEditor().setValue(getEditor().getItems().get(0)));
                    }
                }));
//        settingsPane.getOptions().add(new DefaultOption<>(
//                "图文编辑器", "图文数据默认排版/编辑器", "EDITOR",
//                UserPrefs.prefs.getString("item.article.editor", "advanced"), true,
//                option -> new OptionEditorBase<String, ChoiceBox<String>>(option, new ChoiceBox<>()) {
//                    private StringProperty valueProperty;
//
//                    @Override
//                    public Property<String> valueProperty() {
//                        if (this.valueProperty == null) {
//                            this.valueProperty = new SimpleStringProperty();
//                            this.getEditor().getItems().setAll(
//                                    "simple:简易图文编辑器", "advanced:高级图文/表格编辑器（推荐）"
//                            );
//                            this.getEditor().getSelectionModel().selectedItemProperty().addListener((obs, ov, nv) -> {
//                                if (ov == null || Objects.equals(ov, nv)) return;
//                                this.valueProperty.set(nv);
//                                //
//                                UserPrefs.prefs.setProperty("item.article.editor", nv.split(":", 2)[0]);
//                            });
//                            this.valueProperty.addListener((obs, ov, nv) -> this.setValue(nv));
//                        }
//                        return this.valueProperty;
//                    }
//
//                    @Override
//                    public void setValue(String value) {
//                        if (getEditor().getItems().isEmpty()) return;
//                        getEditor().getItems().stream().filter(v -> v.startsWith(value))
//                                .findFirst()
//                                .ifPresentOrElse(v -> getEditor().setValue(v),
//                                        () -> getEditor().setValue(getEditor().getItems().get(0)));
//                    }
//                }));

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
}
