package org.appxi.smartlib.dao;

import org.appxi.smartlib.item.Item;

import java.io.File;
import java.io.InputStream;
import java.util.List;
import java.util.function.BiConsumer;
import java.util.function.Consumer;
import java.util.zip.ZipFile;
import java.util.zip.ZipOutputStream;

public interface DataAccess {
    List<Item> list(Item parent);

    String walk(Item parent, Consumer<Item> consumer);

    boolean exists(String itemPath);

    List<String> exists(String itemPath, String newParent);

    File filePath(Item item);

    /**
     * @param item
     * @return null for success, otherwise for any error message
     */
    String create(Item item);

    /**
     * @param item
     * @param newName
     * @return null for success, otherwise for any error message
     */
    String rename(Item item, String newName);

    /**
     * @param item
     * @return null for success, otherwise for any error message
     */
    String delete(Item item, BiConsumer<Double, String> progressCallback);

    /**
     * @param item
     * @param content
     * @return null for success, otherwise for any error message
     */
    String setContent(Item item, InputStream content);

    /**
     * @param item
     * @return
     */
    InputStream getContent(Item item);

    String backup(Item parent, ZipOutputStream zipStream, BiConsumer<Double, String> progressCallback);

    String restore(Item parent, ZipFile zipFile, BiConsumer<Double, String> progressCallback);

    String getIdentificationInfo(Item item);

    String move(Item item, Item newParent);

    String reindex(Item item, BiConsumer<Double, String> progressCallback);
}
