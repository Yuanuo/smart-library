package org.appxi.smartlib.item.tika;

public class PptProvider extends TikaProvider {
    public static final PptProvider ONE = new PptProvider();

    private PptProvider() {
    }

    @Override
    public String providerId() {
        return "ppt";
    }

    @Override
    public String providerName() {
        return "PPT";
    }
}
