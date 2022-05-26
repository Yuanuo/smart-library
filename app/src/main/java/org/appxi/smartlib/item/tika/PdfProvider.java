package org.appxi.smartlib.item.tika;

public class PdfProvider extends TikaProvider {
    public static final PdfProvider ONE = new PdfProvider();

    private PdfProvider() {
    }

    @Override
    public String providerId() {
        return "pdf";
    }

    @Override
    public String providerName() {
        return "PDF";
    }
}
