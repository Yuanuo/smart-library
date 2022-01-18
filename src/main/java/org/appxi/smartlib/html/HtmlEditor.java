package org.appxi.smartlib.html;

import javafx.application.Platform;
import javafx.concurrent.Worker;
import javafx.embed.swing.SwingFXUtils;
import javafx.event.EventHandler;
import javafx.scene.Node;
import javafx.scene.image.Image;
import javafx.scene.input.Clipboard;
import javafx.scene.input.KeyCode;
import javafx.scene.input.KeyEvent;
import javafx.scene.web.WebEngine;
import javafx.scene.web.WebView;
import org.apache.commons.io.FilenameUtils;
import org.apache.commons.io.IOUtils;
import org.appxi.holder.RawHolder;
import org.appxi.javafx.app.AppEvent;
import org.appxi.javafx.control.ProgressLayer;
import org.appxi.javafx.helper.FxHelper;
import org.appxi.javafx.visual.VisualEvent;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.prefs.UserPrefs;
import org.appxi.smartlib.AppContext;
import org.appxi.smartlib.item.Item;
import org.appxi.smartlib.item.ItemEditor;
import org.jsoup.Jsoup;
import org.jsoup.nodes.Element;
import org.jsoup.select.NodeVisitor;

import javax.imageio.ImageIO;
import java.io.BufferedInputStream;
import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.InputStream;
import java.net.URL;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.Base64;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.function.Consumer;
import java.util.function.Supplier;
import java.util.stream.Collectors;

public abstract class HtmlEditor extends ItemEditor {
    private final EventHandler<VisualEvent> onSetAppStyle = this::onSetAppStyle;
    private final EventHandler<AppEvent> onAppEventStopping = this::onAppEventStopping;
    private final EventHandler<VisualEvent> onSetWebFont = this::onSetWebFont;

    protected WebView webView;
    protected Runnable progressLayerHandler;

    public HtmlEditor(Item item, WorkbenchPane workbench) {
        super(item, workbench);
    }

    protected abstract WebEngine webEngine();

    @Override
    public void initialize() {
        app.eventBus.addEventHandler(VisualEvent.SET_STYLE, onSetAppStyle);
        app.eventBus.addEventHandler(AppEvent.STOPPING, onAppEventStopping);
        app.eventBus.addEventHandler(VisualEvent.SET_WEB_FONT, onSetWebFont);
    }

    @Override
    public void onViewportShowing(boolean firstTime) {
        if (firstTime) {
            progressLayerHandler = ProgressLayer.show(getViewport(), progressLayer -> FxHelper.runLater(() -> {
                webEngine().setUserDataDirectory(UserPrefs.cacheDir().toFile());
                // apply theme
                this.applyTheme(null);
                //
                webEngine().getLoadWorker().stateProperty().addListener((o, ov, state) -> {
                    if (state == Worker.State.SUCCEEDED) onWebEngineLoadSucceeded();
                    else if (state == Worker.State.FAILED) onWebEngineLoadFailed();
                });
                //
                onWebEngineLoading();
            }));
        } else if (null != webView) {
            Platform.runLater(() -> webView.requestFocus());
        }
    }

    protected abstract void onWebEngineLoading();

    protected void onWebEngineLoadSucceeded() {
        // apply theme
        applyBodyTheme();

        if (null != progressLayerHandler) {
            progressLayerHandler.run();
            progressLayerHandler = null;
        }
    }

    protected void onWebEngineLoadFailed() {
        //
    }

    @Override
    public void onViewportHiding() {
        saveUserExperienceData();
    }

    @Override
    public void onViewportClosing(boolean selected) {
        saveUserExperienceData();
        app.eventBus.removeEventHandler(VisualEvent.SET_STYLE, onSetAppStyle);
        app.eventBus.removeEventHandler(AppEvent.STOPPING, onAppEventStopping);
        app.eventBus.removeEventHandler(VisualEvent.SET_WEB_FONT, onSetWebFont);
        webView = null;
    }

    protected void applyTheme(VisualEvent event) {
        if (null == this.webView) return;

        final RawHolder<byte[]> allBytes = new RawHolder<>();
        allBytes.value = """
                :root {
                    --font-family: tibetan, "%s", AUTO !important;
                    --zoom: %.2f !important;
                }
                """.formatted(app.visualProvider.webFontName(), app.visualProvider.webFontSize())
                .getBytes(StandardCharsets.UTF_8);
        Consumer<InputStream> consumer = stream -> {
            try (BufferedInputStream in = new BufferedInputStream(stream)) {
                int pos = allBytes.value.length;
                byte[] tmpBytes = new byte[pos + in.available()];
                System.arraycopy(allBytes.value, 0, tmpBytes, 0, pos);
                allBytes.value = tmpBytes;
                in.read(allBytes.value, pos, in.available());
            } catch (Exception exception) {
                exception.printStackTrace();
            }
        };
        Optional.ofNullable(VisualEvent.class.getResourceAsStream("web.css")).ifPresent(consumer);
        Optional.ofNullable(AppContext.class.getResourceAsStream("web.css")).ifPresent(consumer);

        String cssData = "data:text/css;charset=utf-8;base64," + Base64.getMimeEncoder().encodeToString(allBytes.value);
        FxHelper.runLater(() -> {
            webEngine().setUserStyleSheetLocation(cssData);
            applyBodyTheme();
        });
    }

    private void applyBodyTheme() {
        if (null == this.webView) return;
        webEngine().executeScript("document.body.setAttribute('class','".concat(app.visualProvider.toString()).concat("')"));
    }

    protected void onSetAppStyle(VisualEvent event) {
        this.applyTheme(event);
    }

    protected void onAppEventStopping(AppEvent event) {
        if (null == this.webView) return;
        saveUserExperienceData();
    }

    protected void onSetWebFont(VisualEvent event) {
        if (null == this.webView) return;
        saveUserExperienceData();
        this.applyTheme(null);
    }

    protected abstract void saveUserExperienceData();

    //////////////////////////////////////////////////////////////////////////////////////////////////

    protected final void attachAdvancedPasteShortcuts(Node node, Supplier<Boolean> editorFocusedSupplier) {
        if (null == node) return;
        node.addEventFilter(KeyEvent.KEY_PRESSED, event -> {
            if (event.isShortcutDown() && event.isShiftDown() && event.getCode() == KeyCode.V) {
                if (!editorFocusedSupplier.get()) return;
                final Clipboard clipboard = Clipboard.getSystemClipboard();
                if (clipboard.hasHtml()) {
                    event.consume();
                    final Element body = Jsoup.parse(clipboard.getHtml()).body();
                    body.traverse(new NodeVisitor() {
                        @Override
                        public void head(org.jsoup.nodes.Node node, int depth) {
                            if (node instanceof Element ele) {
                                ele.removeAttr("id");
                                //
                                if (ele.is("img")) {
                                    wrapImgSrcToBase64Src(ele, true);
                                    return;
                                }
                                if (HtmlHelper.headingTags.contains(ele.tagName()))
                                    ele.tagName("p");
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
                    body.traverse(new NodeVisitor() {
                        @Override
                        public void head(org.jsoup.nodes.Node node, int depth) {
                            if (node instanceof Element ele) {
                                ele.removeAttr("id");
                                //
                                if (ele.is("img"))
                                    wrapImgSrcToBase64Src(ele, false);
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

    protected abstract void insertHtmlAtCursor(String html);

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
