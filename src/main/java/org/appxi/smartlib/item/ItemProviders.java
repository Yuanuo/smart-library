package org.appxi.smartlib.item;

import org.appxi.smartlib.item.article.ArticleProvider;
import org.appxi.smartlib.item.tika.DocProvider;
import org.appxi.smartlib.item.tika.DocxProvider;
import org.appxi.smartlib.item.tika.PdfProvider;
import org.appxi.smartlib.item.tika.PptProvider;
import org.appxi.smartlib.item.tika.PptxProvider;
import org.appxi.smartlib.item.tika.TxtProvider;
import org.appxi.smartlib.item.tika.XlsProvider;
import org.appxi.smartlib.item.tika.XlsxProvider;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.Objects;
import java.util.function.Predicate;

public abstract class ItemProviders {

    private static final List<ItemProvider> PROVIDERS = new ArrayList<>(16);

    static {
        addProviders(ArticleProvider.ONE);
        //
        addProviders(DocxProvider.ONE, DocProvider.ONE,
                PptxProvider.ONE, PptProvider.ONE,
                XlsxProvider.ONE, XlsProvider.ONE,
                PdfProvider.ONE,
                TxtProvider.ONE
        );
        //
        addProviders(FileProvider.ONE, FolderProvider.ONE);
    }

    public static void addProviders(ItemProvider... providers) {
        for (ItemProvider provider : providers)
            if (!PROVIDERS.contains(provider))
                PROVIDERS.add(provider);
    }

    public static void removeProviders(ItemProvider... providers) {
        Arrays.stream(providers).forEach(PROVIDERS::remove);
    }

    public static List<ItemProvider> providers() {
        return List.copyOf(PROVIDERS);
    }

    public static ItemProvider find(String providerId) {
        return find(p -> Objects.equals(p.providerId(), providerId));
    }

    public static ItemProvider find(Predicate<ItemProvider> predicate) {
        for (ItemProvider provider : PROVIDERS) {
            if (predicate.test(provider))
                return provider;
        }
        return null;
    }

    private ItemProviders() {
    }
}
