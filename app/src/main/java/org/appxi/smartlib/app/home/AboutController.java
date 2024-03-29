package org.appxi.smartlib.app.home;

import javafx.geometry.Pos;
import javafx.scene.Node;
import javafx.scene.control.ButtonType;
import javafx.scene.control.Dialog;
import javafx.scene.control.DialogPane;
import javafx.scene.control.Label;
import javafx.scene.control.TextArea;
import javafx.scene.image.ImageView;
import javafx.scene.layout.HBox;
import javafx.scene.layout.Priority;
import javafx.scene.layout.VBox;
import javafx.scene.web.WebView;
import org.appxi.javafx.visual.MaterialIcon;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.javafx.workbench.WorkbenchPart;
import org.appxi.javafx.workbench.WorkbenchPartController;
import org.appxi.prefs.UserPrefs;
import org.appxi.smartlib.app.App;
import org.appxi.util.OSVersions;

import java.util.Optional;

public class AboutController extends WorkbenchPartController implements WorkbenchPart.SideTool {
    public AboutController(WorkbenchPane workbench) {
        super(workbench);

        this.id.set("ABOUT");
        this.title.set("关于");
        this.tooltip.set("关于");
        this.graphic.set(MaterialIcon.INFO_OUTLINE.graphic());
    }

    @Override
    public void postConstruct() {
    }

    @Override
    public void activeViewport(boolean firstTime) {
        final Label head = new Label(App.NAME);
        head.setStyle("-fx-font-size: 2em; -fx-padding: .5em 0;");
        head.setMaxWidth(Double.MAX_VALUE);
        head.setAlignment(Pos.CENTER);
        Optional.ofNullable(App.class.getResource("icon-256.png"))
                .ifPresent(url -> {
                    final ImageView graphic = new ImageView(url.toExternalForm());
                    graphic.setFitWidth(48);
                    graphic.setFitHeight(48);
                    head.setGraphic(graphic);
                    head.setGraphicTextGap(20);
                });

        final HBox headBox = new HBox(head);
        HBox.setHgrow(head, Priority.ALWAYS);

        final Label desc = new Label("包罗万象，重在搜索。一款自维护内容的知识库！");
        desc.setStyle("-fx-font-size: 1.5em; -fx-padding: .5em 0 2em 0;");
        desc.setMaxWidth(Double.MAX_VALUE);
        desc.setAlignment(Pos.CENTER);
        final HBox descBox = new HBox(desc);
        HBox.setHgrow(desc, Priority.ALWAYS);

        final TextArea info = new TextArea();
        VBox.setVgrow(info, Priority.ALWAYS);
        info.setWrapText(true);
        info.setEditable(false);
        info.setPrefRowCount(15);

        final StringBuilder buf = new StringBuilder();
        buf.append("Product Version").append("\n");
        buf.append(App.NAME).append(" ").append(App.VERSION).append("\n\n");

        buf.append("Java Version").append("\n");
        buf.append(System.getProperty("java.runtime.version")).append("\n\n");

        buf.append("JavaFX Version").append("\n");
        buf.append(System.getProperty("javafx.runtime.version")).append("\n\n");

        buf.append("Platform Info").append("\n");
        buf.append(System.getProperty("os.name")).append(", ");
        buf.append(System.getProperty("os.arch")).append(", ");
        buf.append(System.getProperty("os.version")).append("\n\n");

        WebView web = new WebView();
        web.getEngine().setUserDataDirectory(UserPrefs.cacheDir().toFile());

        buf.append("Webview Info").append("\n");
        buf.append(web.getEngine().getUserAgent());

        info.setText(buf.toString());

        //
        final DialogPane dialogPane = new DialogPane() {
            @Override
            protected Node createButtonBar() {
                return null;
            }
        };
        if (OSVersions.isLinux) {
            dialogPane.setPrefSize(540, 720);
        }
        dialogPane.setContent(new VBox(headBox, descBox, info));
        dialogPane.getButtonTypes().add(ButtonType.OK);
        //
        final Dialog<?> dialog = new Dialog<>();
        dialog.setTitle(title.get());
        dialog.setDialogPane(dialogPane);
        if (OSVersions.isLinux) {
            dialog.setResizable(true);
        }
        dialog.initOwner(app.getPrimaryStage());
        dialog.show();
    }
}
