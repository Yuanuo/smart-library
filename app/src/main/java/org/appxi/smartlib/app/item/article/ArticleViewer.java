package org.appxi.smartlib.app.item.article;

import javafx.scene.control.Button;
import org.appxi.javafx.visual.MaterialIcon;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.smartlib.ItemEvent;
import org.appxi.smartlib.app.item.HtmlBasedViewer;
import org.appxi.smartlib.app.item.ItemEx;
import org.appxi.smartlib.article.ArticleDocument;
import org.appxi.util.StringHelper;

import java.nio.file.Path;

public class ArticleViewer extends HtmlBasedViewer {
    final ArticleDocument document;

    public ArticleViewer(WorkbenchPane workbench, ItemEx item) {
        super(workbench, item);
        this.document = new ArticleDocument(item);
    }

    @Override
    public void initialize() {
        super.initialize();
        //
        addTool_EditArticle();
    }

    protected void addTool_EditArticle() {
        Button button = new Button("编辑");
        button.getStyleClass().addAll("flat");
        button.setGraphic(MaterialIcon.EDIT.graphic());
        button.setOnAction(event -> app.eventBus.fireEvent(new ItemEvent(ItemEvent.EDITING, item)));
        //
        this.webPane.getTopBar().addLeft(button);
    }

    @Override
    protected Object createWebContent() {
        String htmlFile = this.document.toViewableHtmlFile(null,
                body -> StringHelper.concat("<body><article>", body.html(), "</article></body>"),
                HtmlBasedViewer.getWebIncludeURIsEx().toArray(new String[0])
        );
        return Path.of(htmlFile);
    }
}
