package org.appxi.smartlib.item.tika;

import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.smartlib.html.HtmlViewer;
import org.appxi.smartlib.item.Item;
import org.appxi.util.StringHelper;

import java.nio.file.Path;

class TikaViewer extends HtmlViewer {
    final TikaProvider itemProvider;

    public TikaViewer(Item item, WorkbenchPane workbench) {
        super(item, workbench);
        this.itemProvider = (TikaProvider) item.provider;
    }

    @Override
    protected Path createViewableHtmlFile() {
        String htmlFile = ((TikaProvider) item.provider).toViewableHtmlFile(this.item, null,
                body -> StringHelper.concat("<body><article>", body.html(), "</article></body>"),
                WebIncl.getIncludePaths()
        );
        return Path.of(htmlFile);
    }
}
