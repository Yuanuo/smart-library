package org.appxi.smartlib.app.item.tika;

import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.smartlib.Item;
import org.appxi.smartlib.app.html.HtmlViewer;
import org.appxi.smartlib.tika.TikaProvider;
import org.appxi.util.StringHelper;

import java.nio.file.Path;

public class TikaViewer extends HtmlViewer {
    final TikaProvider itemProvider;

    public TikaViewer(Item item, WorkbenchPane workbench) {
        super(item, workbench);
        this.itemProvider = (TikaProvider) item.provider;
    }

    @Override
    protected Object prepareHtmlContent() {
        String htmlFile = ((TikaProvider) item.provider).toViewableHtmlFile(this.item, null,
                body -> StringHelper.concat("<body><article>", body.html(), "</article></body>"),
                WebIncl.getIncludePaths()
        );
        return Path.of(htmlFile).toUri();
    }
}
