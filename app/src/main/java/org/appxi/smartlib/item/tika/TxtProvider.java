package org.appxi.smartlib.item.tika;

public class TxtProvider extends TikaProvider {
    public static final TxtProvider ONE = new TxtProvider();

    private TxtProvider() {
    }

    @Override
    public String providerId() {
        return "txt";
    }

    @Override
    public String providerName() {
        return "TXT";
    }
}
