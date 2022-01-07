package org.appxi.smartlib.item.article;

import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.smartlib.html.AdvancedEditor;
import org.appxi.smartlib.item.Item;

class ArticleEditor extends AdvancedEditor {
    final ArticleDocument document;

    public ArticleEditor(Item item, WorkbenchPane workbench) {
        super(item, workbench);
        this.document = new ArticleDocument(item);
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

    @Override
    protected void editMetadata() {
        new ArticleMetadata(document).showDialog();
    }
}
