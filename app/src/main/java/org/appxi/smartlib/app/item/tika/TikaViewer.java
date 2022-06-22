package org.appxi.smartlib.app.item.tika;

import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.smartlib.app.item.HtmlBasedViewer;
import org.appxi.smartlib.app.item.ItemEx;
import org.appxi.smartlib.tika.TikaProvider;
import org.appxi.util.StringHelper;

import java.nio.file.Path;

public class TikaViewer extends HtmlBasedViewer {
    final TikaProvider itemProvider;

    public TikaViewer(WorkbenchPane workbench, ItemEx item) {
        super(workbench, item);
        this.itemProvider = (TikaProvider) item.provider;
    }

    @Override
    protected Object createWebContent() {
        String htmlFile = ((TikaProvider) item.provider).toViewableHtmlFile(this.item, null,
                body -> StringHelper.concat("<body><article>", body.html(), "</article></body>"),
                HtmlBasedViewer.getWebIncludeURIsEx().toArray(new String[0])
        );
        return Path.of(htmlFile);
    }
}
