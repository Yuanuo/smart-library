package org.appxi.smartlib.item.tika;

public class PptxProvider extends TikaProvider {
    public static final PptxProvider ONE = new PptxProvider();

    private PptxProvider() {
    }

    @Override
    public String providerId() {
        return "pptx";
    }

    @Override
    public String providerName() {
        return "PPTX";
    }
}
