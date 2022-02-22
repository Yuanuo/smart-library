package org.appxi.smartlib.item.article;

import javafx.scene.layout.StackPane;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.smartlib.html.HtmlEditor;
import org.appxi.smartlib.item.Item;

class ArticleEditor extends HtmlEditor {
    final ArticleDocument document;

    public ArticleEditor(Item item, WorkbenchPane workbench) {
        super(item, workbench);
        this.document = new ArticleDocument(item);
    }

    @Override
    protected void onViewportInitOnce(StackPane viewport) {
        super.onViewportInitOnce(viewport);
        //
        addEdit_Metadata(document);
    }

    @Override
    protected String loadEditorContent() {
        return this.document.body().html();
    }

    @Override
    protected void saveEditorContent(String content) {
        document.setDocumentBody(content);
        document.save();
    }
}
