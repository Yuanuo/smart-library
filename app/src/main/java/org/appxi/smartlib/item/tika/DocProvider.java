package org.appxi.smartlib.item.tika;

public class DocProvider extends TikaProvider {
    public static final DocProvider ONE = new DocProvider();

    private DocProvider() {
    }

    @Override
    public String providerId() {
        return "doc";
    }

    @Override
    public String providerName() {
        return "DOC";
    }
}
