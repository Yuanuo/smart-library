package org.appxi.smartlib.app.item.article;

import javafx.scene.control.Button;
import javafx.scene.layout.StackPane;
import org.appxi.javafx.visual.MaterialIcon;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.smartlib.Item;
import org.appxi.smartlib.ItemEvent;
import org.appxi.smartlib.app.html.HtmlViewer;
import org.appxi.smartlib.article.ArticleDocument;
import org.appxi.util.StringHelper;

import java.nio.file.Path;

public class ArticleViewer extends HtmlViewer {
    final ArticleDocument document;

    public ArticleViewer(Item item, WorkbenchPane workbench) {
        super(item, workbench);
        this.document = new ArticleDocument(item);
    }

    @Override
    protected void onViewportInitOnce(StackPane viewport) {
        super.onViewportInitOnce(viewport);

        addTool_EditArticle();
    }

    protected void addTool_EditArticle() {
        Button button = new Button("编辑");
        button.getStyleClass().addAll("flat");
        button.setGraphic(MaterialIcon.EDIT.graphic());
        button.setOnAction(event -> app.eventBus.fireEvent(new ItemEvent(ItemEvent.EDITING, item)));
        //
        webPane().getTopAsBar().addLeft(button);
    }

    @Override
    protected Object prepareHtmlContent() {
        String htmlFile = this.document.toViewableHtmlFile(null,
                body -> StringHelper.concat("<body><article>", body.html(), "</article></body>"),
                WebIncl.getIncludePaths()
        );
        return Path.of(htmlFile).toUri();
    }
}
