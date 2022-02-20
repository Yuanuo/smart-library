package org.appxi.smartlib.html;

import javafx.concurrent.Worker;
import javafx.event.Event;
import javafx.event.EventHandler;
import javafx.scene.layout.StackPane;
import netscape.javascript.JSObject;
import org.appxi.holder.RawHolder;
import org.appxi.javafx.app.AppEvent;
import org.appxi.javafx.control.ProgressLayer;
import org.appxi.javafx.control.WebPane;
import org.appxi.javafx.helper.FxHelper;
import org.appxi.javafx.visual.VisualEvent;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.prefs.UserPrefs;
import org.appxi.smartlib.AppContext;
import org.appxi.smartlib.item.Item;
import org.appxi.smartlib.item.ItemRenderer;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.io.BufferedInputStream;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.util.Base64;
import java.util.Optional;
import java.util.function.Consumer;

public abstract class HtmlRenderer extends ItemRenderer {
    protected static final Logger logger = LoggerFactory.getLogger(HtmlRenderer.class);

    private final EventHandler<VisualEvent> onSetAppStyle = this::onSetAppStyle;
    private final EventHandler<AppEvent> onAppEventStopping = this::onAppEventStopping;
    private final EventHandler<VisualEvent> onSetWebStyle = this::onSetWebStyle;

    private WebPane webPane;
    protected Runnable progressLayerHandler;

    public HtmlRenderer(Item item, WorkbenchPane workbench) {
        super(item, workbench);
    }

    protected final WebPane webPane() {
        return webPane;
    }

    @Override
    public void initialize() {
        app.eventBus.addEventHandler(VisualEvent.SET_STYLE, onSetAppStyle);
        app.eventBus.addEventHandler(AppEvent.STOPPING, onAppEventStopping);
        app.eventBus.addEventHandler(VisualEvent.SET_WEB_STYLE, onSetWebStyle);
    }

    @Override
    protected void onViewportInitOnce(StackPane viewport) {
        viewport.getChildren().add(this.webPane = new WebPane());
    }

    @Override
    public void onViewportShowing(boolean firstTime) {
        if (firstTime) {
            progressLayerHandler = ProgressLayer.show(getViewport(), progressLayer -> FxHelper.runLater(() -> {
                webPane.webEngine().setUserDataDirectory(UserPrefs.cacheDir().toFile());
                // apply theme
                this.applyWebStyle(null);
                //
                webPane.webEngine().getLoadWorker().stateProperty().addListener((o, ov, state) -> {
                    if (state == Worker.State.SUCCEEDED) onWebEngineLoadSucceeded();
                    else if (state == Worker.State.FAILED) onWebEngineLoadFailed();
                });
                //
                onWebEngineLoading();
            }));
        } else if (null != webPane) {
            FxHelper.runLater(() -> {
                if (null != webPane) webPane.webView().requestFocus();
            });
        }
    }

    protected abstract void onWebEngineLoading();

    protected void onWebEngineLoadSucceeded() {
        // set an interface object named 'console' in the web engine's page for debug
        final JSObject window = webPane.executeScript("window");
        window.setMember("console", consoleWrapper);
        // apply theme
        applyWebStyleByBodyClass();
        //
        webPane().patch();
    }

    protected void onWebEngineLoadFailed() {
        logger.warn("onWebEngineLoadFailed");
    }

    @Override
    public void onViewportHiding() {
        saveUserExperienceData();
    }

    @Override
    public void onViewportClosing(Event event, boolean selected) {
        saveUserExperienceData();
        app.eventBus.removeEventHandler(VisualEvent.SET_STYLE, onSetAppStyle);
        app.eventBus.removeEventHandler(AppEvent.STOPPING, onAppEventStopping);
        app.eventBus.removeEventHandler(VisualEvent.SET_WEB_STYLE, onSetWebStyle);
        if (null != webPane) {
            webPane.release();
            webPane = null;
        }
    }

    protected void saveUserExperienceData() {
    }

    protected void applyWebStyle(VisualEvent event) {
        if (null == this.webPane) return;

        final RawHolder<byte[]> allBytes = new RawHolder<>();
        allBytes.value = """
                :root {
                    --font-family: tibetan, "%s", AUTO !important;
                    --zoom: %.2f !important;
                    --text-color: %s;
                }
                body {
                    background-color: %s;
                }
                """.formatted(app.visualProvider.webFontName(),
                app.visualProvider.webFontSize(),
                app.visualProvider.webTextColor(),
                app.visualProvider.webPageColor()
        ).getBytes(StandardCharsets.UTF_8);
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
            webPane.webEngine().setUserStyleSheetLocation(cssData);
            applyWebStyleByBodyClass();
        });
    }

    private void applyWebStyleByBodyClass() {
        if (null == webPane) return;
        try {
            webPane.executeScript("document.body.setAttribute('class','".concat(app.visualProvider.toString()).concat("');"));
        } catch (RuntimeException e) {
            logger.warn("applyWebStyle by body class failed", e);
        }
    }

    protected void onSetAppStyle(VisualEvent event) {
        this.applyWebStyle(null);
    }

    protected void onAppEventStopping(AppEvent event) {
        if (null == this.webPane) return;
        saveUserExperienceData();
    }

    protected void onSetWebStyle(VisualEvent event) {
        if (null == this.webPane) return;
        saveUserExperienceData();
        this.applyWebStyle(null);
        navigate(null);
    }

    /**
     * for communication from the Javascript engine.
     */
    private final ConsoleWrapper consoleWrapper = new ConsoleWrapper();

    public static final class ConsoleWrapper {
        private ConsoleWrapper() {
        }

        public void log(String msg) {
            logger.warn(msg);
        }
    }
}
