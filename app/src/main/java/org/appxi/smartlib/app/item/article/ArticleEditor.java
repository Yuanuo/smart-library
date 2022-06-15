package org.appxi.smartlib.app.item.article;

import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.smartlib.app.item.HtmlBasedEditor;
import org.appxi.smartlib.app.item.ItemEx;
import org.appxi.smartlib.article.ArticleDocument;

public class ArticleEditor extends HtmlBasedEditor {
    final ArticleDocument document;

    public ArticleEditor(WorkbenchPane workbench, ItemEx item) {
        super(workbench, null, item);
        this.document = new ArticleDocument(item);
    }

    @Override
    public void postConstruct() {
    }

    @Override
    public void initialize() {
        super.initialize();
        //
        addEdit_Renamer();
        addEdit_Metadata(document);
    }

    @Override
    protected String loadEditorContent() {
        return this.document.body().html();
    }

    @Override
    protected void saveEditorContent(String content) throws Exception {
        document.setDocumentBody(content);
        document.save();
    }
}
