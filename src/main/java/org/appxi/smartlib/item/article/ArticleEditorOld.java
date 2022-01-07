package org.appxi.smartlib.item.article;

import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.smartlib.html.SimpleEditor;
import org.appxi.smartlib.item.Item;

class ArticleEditorOld extends SimpleEditor {
    final ArticleDocument document;

    public ArticleEditorOld(Item item, WorkbenchPane workbench) {
        super(item, workbench);
        this.document = new ArticleDocument(item);
    }

    @Override
    protected void saveEditorContent(String content) {
        this.document.setDocumentBody(content);
        this.document.save();
    }

    @Override
    protected String loadEditorContent() {
        return this.document.body().outerHtml();
    }

    @Override
    protected void editMetadata() {
        if (new ArticleMetadata(document).showDialog())
            documentChanges.set(documentChanges.get() + 2);
    }
}
