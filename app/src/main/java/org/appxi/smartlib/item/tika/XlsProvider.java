package org.appxi.smartlib.item.tika;

public class XlsProvider extends TikaProvider {
    public static final XlsProvider ONE = new XlsProvider();

    private XlsProvider() {
    }

    @Override
    public String providerId() {
        return "xls";
    }

    @Override
    public String providerName() {
        return "XLS";
    }
}
