package org.appxi.smartlib.item.tika;

public class XlsxProvider extends TikaProvider {
    public static final XlsxProvider ONE = new XlsxProvider();

    private XlsxProvider() {
    }

    @Override
    public String providerId() {
        return "xlsx";
    }

    @Override
    public String providerName() {
        return "XLSX";
    }
}
