package org.appxi.smartlib.html;

import javafx.beans.binding.Bindings;
import javafx.beans.value.ChangeListener;
import javafx.embed.swing.SwingFXUtils;
import javafx.event.Event;
import javafx.geometry.Pos;
import javafx.scene.Node;
import javafx.scene.control.Button;
import javafx.scene.control.TextField;
import javafx.scene.control.Tooltip;
import javafx.scene.image.Image;
import javafx.scene.input.Clipboard;
import javafx.scene.input.DataFormat;
import javafx.scene.input.KeyCode;
import javafx.scene.input.KeyEvent;
import javafx.scene.layout.HBox;
import javafx.scene.layout.Priority;
import org.apache.commons.io.FilenameUtils;
import org.apache.commons.io.IOUtils;
import org.appxi.javafx.control.ProgressLayer;
import org.appxi.javafx.helper.FxHelper;
import org.appxi.javafx.visual.MaterialIcon;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.smartlib.App;
import org.appxi.smartlib.item.Item;
import org.appxi.smartlib.item.ItemEvent;
import org.appxi.util.FileHelper;
import org.appxi.util.StringHelper;
import org.jsoup.Jsoup;
import org.jsoup.nodes.Element;
import org.jsoup.select.NodeVisitor;

import javax.imageio.ImageIO;
import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.InputStream;
import java.net.URL;
import java.nio.file.Files;
import java.util.Base64;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.function.Supplier;
import java.util.stream.Collectors;

public abstract class HtmlRendererEx extends HtmlRenderer {
    public HtmlRendererEx(Item item, WorkbenchPane workbench) {
        super(item, workbench);
    }

    /* //////////////////////////////////////////////////////////////////// */

    protected void bindPropertiesForEdit() {
        this.id.bind(Bindings.createStringBinding(() -> "Edit@".concat(item.typedPath()), item.path));
        this.title.bind(Bindings.createStringBinding(() -> "编辑: ".concat(item.getName()), item.name));
        this.tooltip.bind(Bindings.createStringBinding(() -> "编辑: ".concat(item.typedPath()), item.path));
    }

    protected void initTopAreaForEdit() {
        HBox topArea = new HBox(8);
        topArea.setAlignment(Pos.CENTER_LEFT);
        topArea.setStyle("-fx-padding: .5em;");
        this.webPane().setTop(topArea);
        //
        edit_NameEdit(topArea);
        edit_Metadata(topArea);
    }

    private ChangeListener<String> nameChangeListener;

    private void edit_NameEdit(HBox topArea) {
        final TextField nameEditor = new TextField(item.getName());
        nameEditor.setPromptText("在此输入");
        nameEditor.setTooltip(new Tooltip("在此处输入后按回车应用修改"));
        HBox.setHgrow(nameEditor, Priority.ALWAYS);
        nameChangeListener = (o, ov, nv) -> nameEditor.setText(nv);
        item.name.addListener(nameChangeListener);
        nameEditor.addEventHandler(KeyEvent.KEY_RELEASED, event -> {
            if (event.getCode() == KeyCode.ENTER) {
                event.consume();
                String inputText = FileHelper.toValidName(nameEditor.getText()).strip().replaceAll("^[.]", "");
                if (inputText.isBlank()) {
                    nameEditor.setText(item.getName());
                    return;
                }
                String targetName = StringHelper.trimChars(inputText, 80);
                if (targetName.equals(item.getName()))
                    return;

                final Item oldItem = item.clone(), newItem = item.clone();
                final String newName = targetName;
                ProgressLayer.showAndWait(getViewport(), progressLayer -> {
                    final String msg = org.appxi.smartlib.dao.DataApi.dataAccess().rename(newItem, newName);
                    if (msg != null) {
                        app.toastError(msg);
                        return;
                    }
                    FxHelper.runLater(() -> {
                        item.setName(newItem.getName()).setPath(newItem.getPath());
                        //
                        App.app().eventBus.fireEvent(new ItemEvent(ItemEvent.RENAMED, item, oldItem));
                        app.toast("重命名成功");
                    });
                });
            }
        });
        topArea.getChildren().addAll(nameEditor);
    }

    private void edit_Metadata(HBox topArea) {
        final Button button = new Button("元数据");
        button.setTooltip(new Tooltip("编辑此文档的类目、作者等信息"));
        button.setGraphic(MaterialIcon.STYLE.graphic());
        button.setOnAction(event -> editMetadata());
        topArea.getChildren().add(button);
    }

    protected void editMetadata() {
        throw new UnsupportedOperationException("Not implements");
    }

    @Override
    public void onViewportClosing(Event event, boolean selected) {
        super.onViewportClosing(event, selected);
        //
        if (!event.isConsumed() && null != this.nameChangeListener)
            item.name.removeListener(this.nameChangeListener);
    }

    //////////////////////////////////////////////////////////////////////////////////////////////////

    protected final void attachAdvancedPasteShortcuts(Node node, Supplier<Boolean> editorFocusedSupplier) {
        if (null == node) return;
        node.addEventFilter(KeyEvent.KEY_PRESSED, event -> {
            if (event.isShortcutDown() && event.isShiftDown() && event.isAltDown() && event.getCode() == KeyCode.V) {
                final Clipboard clipboard = Clipboard.getSystemClipboard();
                if (clipboard.hasHtml()) {
                    event.consume();
                    Clipboard.getSystemClipboard().setContent(Map.of(DataFormat.PLAIN_TEXT, clipboard.getHtml()));
                }
            } else if (event.isShortcutDown() && event.isShiftDown() && event.getCode() == KeyCode.V) {
                if (!editorFocusedSupplier.get()) return;
                final Clipboard clipboard = Clipboard.getSystemClipboard();
                if (clipboard.hasHtml()) {
                    event.consume();
                    final Element body = Jsoup.parse(clipboard.getHtml()).body();
                    Optional.ofNullable(body.children().first())
                            .filter(ele -> "div".equals(ele.tagName())).ifPresent(Element::unwrap);
                    body.traverse(new NodeVisitor() {
                        @Override
                        public void head(org.jsoup.nodes.Node node, int depth) {
                            if (depth == 0) return;
                            if (node instanceof Element ele) {
                                ele.removeAttr("id");
                                //
                                if (depth == 1 && "div".equals(ele.tagName())) {
                                    ele.tagName("p");
                                } else if ("img".equals(ele.tagName())) {
                                    wrapImgSrcToBase64Src(ele, true);
                                    return;
                                } else if (HtmlHelper.headingTags.contains(ele.tagName())) {
                                    ele.tagName("p");
                                }
                                ele.clearAttributes();
                            }
                        }

                        @Override
                        public void tail(org.jsoup.nodes.Node node, int depth) {
                        }
                    });
                    HtmlHelper.inlineFootnotes(body);
                    insertHtmlAtCursor(body.html());
                } else if (clipboard.hasString()) {
                    event.consume();
                    String str = clipboard.getString().lines()
                            .map(s -> "<p>".concat(s.strip()).concat("</p>"))
                            .collect(Collectors.joining("\n"));
                    final Element body = Jsoup.parse(str).body();
                    HtmlHelper.inlineFootnotes(body);
                    insertHtmlAtCursor(body.html());
                }
            } else if (event.isShortcutDown() && event.getCode() == KeyCode.V) {
                if (!editorFocusedSupplier.get()) return;
                final Clipboard clipboard = Clipboard.getSystemClipboard();
                if (clipboard.hasFiles()) {
                    event.consume();
                    for (File file : clipboard.getFiles()) {
                        if (FilenameUtils.isExtension(file.getName().toLowerCase(), "jpg", "png", "jpeg", "gif")) {
                            insertImageFileAtCursor(file);
                        }
                    }
                } else if (clipboard.hasHtml()) {
                    event.consume();
                    final Element body = Jsoup.parse(clipboard.getHtml()).body();
                    Optional.ofNullable(body.children().first())
                            .filter(ele -> "div".equals(ele.tagName())).ifPresent(Element::unwrap);
                    body.traverse(new NodeVisitor() {
                        @Override
                        public void head(org.jsoup.nodes.Node node, int depth) {
                            if (depth == 0) return;
                            if (node instanceof Element ele) {
                                ele.removeAttr("id");
                                //
                                if (depth == 1 && "div".equals(ele.tagName())) ele.tagName("p");
                                else if ("img".equals(ele.tagName())) wrapImgSrcToBase64Src(ele, false);
                                else if (HtmlHelper.headingTags.contains(ele.tagName())) ele.tagName("p");
                            }
                        }

                        @Override
                        public void tail(org.jsoup.nodes.Node node, int depth) {
                        }
                    });
                    HtmlHelper.inlineFootnotes(body);
                    insertHtmlAtCursor(body.html());
                } else if (clipboard.hasString()) {
                    event.consume();
                    insertHtmlAtCursor(clipboard.getString());
                } else if (clipboard.hasImage()) {
                    event.consume();
                    final Image image = clipboard.getImage();
                    try (ByteArrayOutputStream buff = new ByteArrayOutputStream()) {
                        ImageIO.write(SwingFXUtils.fromFXImage(image, null), "png", buff);
                        insertHtmlAtCursor(wrapImageToBase64Img("png", (int) image.getWidth(), buff.toByteArray()));
                    } catch (Exception e) {
                        e.printStackTrace();
                    }
                }
            }
        });
    }

    protected final void insertImageFileAtCursor(File file) {
        if (file.length() > 10 * 1024 * 1024) {
            app.toast("无法添加大于10MB的文件，请使用小文件！");
            return;
        }
        try {
            final Image image = new Image(file.toURI().toString());

            final String imageType = FilenameUtils.getExtension(file.getName()).toLowerCase();
            final int imageWidth = (int) image.getWidth();
            final byte[] imageBytes = Files.readAllBytes(file.toPath());

            insertHtmlAtCursor(wrapImageToBase64Img(imageType, imageWidth, imageBytes));
        } catch (Exception e) {
            e.printStackTrace();
            app.toast("无法读取所选图片文件，请更换重试！");
        }
    }

    protected void insertHtmlAtCursor(String html) {
        throw new UnsupportedOperationException("Not implements");
    }

    private String wrapImageToBase64Img(String imageType, int imageWidth, byte[] imageBytes) {
        try {
            return "<img align=\"center\" alt=\"img\" style=\"width:100%;max-width:"
                    .concat(String.valueOf(imageWidth))
                    .concat("px\" src=\"").concat(wrapImageToBase64Src(imageType, imageBytes)).concat("\" />");
        } catch (Exception e) {
            e.printStackTrace();
        }
        return null;
    }

    private String wrapImageToBase64Src(String imageType, byte[] imageBytes) {
        return "data:image/".concat(imageType).concat(";base64,").concat(Base64.getEncoder().encodeToString(imageBytes));
    }

    private void wrapImgSrcToBase64Src(Element ele, boolean cleanAttributes) {
        if (!ele.is("img") || !ele.hasAttr("src"))
            return;
        final String src = ele.attr("src");
        if (cleanAttributes) {
            List.copyOf(ele.attributes().asList()).forEach(a -> ele.removeAttr(a.getKey()));
        }
        final String srcTmp = src.toLowerCase();
        final Map<Integer, String> srcTypes = new HashMap<>();
        srcTypes.put(srcTmp.indexOf(".jpg"), "jpg");
        srcTypes.put(srcTmp.indexOf(".jpeg"), "jpeg");
        srcTypes.put(srcTmp.indexOf(".png"), "png");
        srcTypes.put(srcTmp.indexOf(".gif"), "gif");
        srcTypes.put(srcTmp.indexOf("=jpg"), "jpg");
        srcTypes.put(srcTmp.indexOf("=jpeg"), "jpeg");
        srcTypes.put(srcTmp.indexOf("=png"), "png");
        srcTypes.put(srcTmp.indexOf("=gif"), "gif");
        srcTypes.put(srcTmp.indexOf("_jpg"), "jpg");
        srcTypes.put(srcTmp.indexOf("_jpeg"), "jpeg");
        srcTypes.put(srcTmp.indexOf("_png"), "png");
        srcTypes.put(srcTmp.indexOf("_gif"), "gif");
        srcTypes.keySet().stream().sorted().filter(v -> v != -1).findFirst()
                .ifPresent(v -> {
                    Image image = new Image(src);
                    String imageType = srcTypes.get(v);
                    int imageWidth = (int) image.getWidth();
                    // TODO 有些链接防盗链，无法获取到图片
                    if (imageWidth > 0) {
                        try (InputStream srcStream = new URL(src).openStream()) {
                            byte[] imageBytes = IOUtils.readFully(srcStream, srcStream.available());
                            ele.attr("src", wrapImageToBase64Src(imageType, imageBytes));
                            ele.attr("style", "width:100%;max-width:" + imageWidth + "px");
                        } catch (Exception exception) {
                            exception.printStackTrace();
                        }
                    } else {
                        app.toast("网络图片被防盗链阻止，无法获取！");
                        ele.attr("src", src);
                    }
                });
    }
}
