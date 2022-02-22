package org.appxi.smartlib.item.mindmap;

import org.apache.commons.io.IOUtils;
import org.appxi.smartlib.App;
import org.appxi.smartlib.dao.DataApi;
import org.appxi.smartlib.explorer.ItemActions;
import org.appxi.smartlib.item.Item;
import org.appxi.smartlib.item.MetadataApi;
import org.appxi.smartlib.search.Searchable;
import org.appxi.util.DigestHelper;
import org.appxi.util.StringHelper;
import org.json.JSONArray;
import org.json.JSONObject;

import java.io.ByteArrayInputStream;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.util.List;
import java.util.function.Consumer;

class MindmapDocument implements MetadataApi {
    static {
        JSONObject.writeSingleLine = false;
    }

    final Item item;
    private JSONObject document, metadata;

    MindmapDocument(Item item) {
        this.item = item;
    }

    final JSONObject getMetadata() {
        if (null == metadata) getDocument();
        return metadata;
    }

    final JSONObject getDocument() {
        if (null != this.document) return document;

        try (InputStream stream = DataApi.dataAccess().getContent(this.item)) {
            this.document = new JSONObject(IOUtils.toString(stream, StandardCharsets.UTF_8));
        } catch (Throwable ignore) {
        }
        if (null == this.document) this.document = new JSONObject();
        //
        if (!document.has("root")) document.put("root", new JSONObject("""
                {
                    "data": {
                        "id": "%s",
                        "created": %d,
                        "text": "Main Topic / 中心主题"
                    }
                }
                """.formatted(DigestHelper.uid(), System.currentTimeMillis())));
        if (!document.has("template")) document.put("template", "default");
        if (!document.has("theme")) document.put("theme", "fresh-blue");
        if (!document.has("version")) document.put("version", "21.8.18");

        if (document.has("metadata") && document.remove("metadata") instanceof JSONObject meta) this.metadata = meta;
        else this.metadata = new JSONObject();

        if (StringHelper.isBlank(metadata.optString("id"))) metadata.put("id", DigestHelper.uid62s());

        //
        item.attr(Searchable.class, Searchable.of(getMetadata("searchable", "all")));
        //
        return document;
    }

    void setDocumentBody(String json) {
        try {
            this.document.clear();
            this.document = new JSONObject(json);
        } catch (Throwable t) {
            if (!App.productionMode) t.printStackTrace();
        }
    }

    void save() {
        this.save(true);
    }

    void save(boolean reindex) {
        this.document.put("metadata", this.metadata);
        //
        ItemActions.setContent(this.item, new ByteArrayInputStream(this.document.toString(1).getBytes(StandardCharsets.UTF_8)), reindex);
    }

    String id() {
        return getMetadata().getString("id");
    }

    @Override
    public Item item() {
        return this.item;
    }

    @Override
    public List<String> getMetadata(String key) {
        JSONArray array = getMetadata().optJSONArray(key);
        if (null == array) array = new JSONArray();
        return array.toList().stream().map(v -> v.toString().strip()).filter(v -> !v.isBlank()).distinct().sorted().toList();
    }

    @Override
    public String getMetadata(String key, String defaultValue) {
        JSONArray array = getMetadata().optJSONArray(key);
        return (null == array || array.isEmpty()) ? defaultValue : array.optString(0, defaultValue);
    }

    @Override
    public void setMetadata(String key, String value) {
        JSONArray array = getMetadata().optJSONArray(key);
        if (null == array) getMetadata().put(key, array = new JSONArray());
        array.clear();
        array.put(value);
    }

    @Override
    public void addMetadata(String key, String value) {
        JSONArray array = getMetadata().optJSONArray(key);
        if (null == array) getMetadata().put(key, array = new JSONArray());
        array.put(value);
    }

    @Override
    public void removeMetadata(String key) {
        getMetadata().remove(key);
    }

    ////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

    List<MindmapDocument> toSearchableDocuments() {
        return List.of(this);
    }

    String getDocumentText() {
        try {
            StringBuilder buff = new StringBuilder();
            JSONObject root = getDocument().getJSONObject("root");
            walkJson(root, json -> buff.append(json.optString("text")).append("\n"));
            return buff.toString();
        } catch (Throwable t) {
            t.printStackTrace();
        }
        return null;
    }

    void walkJson(JSONObject json, Consumer<JSONObject> consumer) {
        JSONObject data = json.optJSONObject("data");
        if (null != data) consumer.accept(data);

        JSONArray array = json.optJSONArray("children");
        if (null != array && !array.isEmpty()) {
            for (int i = 0; i < array.length(); i++) {
                walkJson(array.optJSONObject(i), consumer);
            }
        }
    }
}
