package org.appxi.smartlib.app.recent;

import javafx.scene.input.KeyCode;
import javafx.scene.input.KeyCodeCombination;
import org.appxi.holder.RawHolder;
import org.appxi.javafx.app.AppEvent;
import org.appxi.javafx.app.DesktopApp;
import org.appxi.javafx.helper.FxHelper;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.javafx.workbench.WorkbenchPart;
import org.appxi.javafx.workbench.WorkbenchPartController;
import org.appxi.prefs.Preferences;
import org.appxi.prefs.PreferencesInProperties;
import org.appxi.prefs.UserPrefs;
import org.appxi.smartlib.FileProvider;
import org.appxi.smartlib.Item;
import org.appxi.smartlib.ItemProviders;
import org.appxi.smartlib.app.explorer.ItemActions;
import org.appxi.smartlib.app.item.HtmlBasedViewer;
import org.appxi.smartlib.app.item.ItemEx;
import org.appxi.smartlib.app.item.ItemRenderer;
import org.appxi.smartlib.dao.ItemsDao;

import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Objects;

public class RecentViewsController extends WorkbenchPartController {
    public RecentViewsController(WorkbenchPane workbench) {
        super(workbench);
    }

    @Override
    public void initialize() {
        app.eventBus.addEventHandler(AppEvent.STOPPING, event -> saveRecentViews());
        app.getPrimaryScene().getAccelerators().put(new KeyCodeCombination(KeyCode.F1), this::showWelcome);
        //
        final RawHolder<WorkbenchPart.MainView> swapRecentViewSelected = new RawHolder<>();
        final List<WorkbenchPart.MainView> swapRecentViews = new ArrayList<>();
        app.eventBus.addEventHandler(AppEvent.STARTING, event -> {
            final Preferences recents = createRecentViews(true);
            WorkbenchPart.MainView addedController = null;

            for (String key : recents.getPropertyKeys()) {
                final String[] arr = recents.getString(key, "").split("\\|", 3);
                if (arr.length != 3) continue;

                if (!ItemsDao.items().exists(key))
                    continue;

                final String pid = "null".equals(arr[0]) ? null : arr[0];
                final Item item = new ItemEx(arr[2], key,
                        ItemProviders.find(p -> !p.isFolder() && Objects.equals(p.providerId(), pid)));

                if (ItemActions.hasViewer(item.provider)) {
                    addedController = ItemActions.getViewer(item.provider).apply(item);
                    if (null == addedController) continue;
                    if ("true".equals(arr[1]))
                        swapRecentViewSelected.value = addedController;
                    swapRecentViews.add(addedController);
                }
            }
            if (!swapRecentViews.isEmpty()) {
                FxHelper.runLater(() -> {
                    for (WorkbenchPart.MainView viewController : swapRecentViews) {
                        workbench.addWorkbenchPartAsMainView(viewController, true);
                    }
                });
                if (null == swapRecentViewSelected.value)
                    swapRecentViewSelected.value = addedController;
            }
        });
        app.eventBus.addEventHandler(AppEvent.STARTED, event -> FxHelper.runThread(100, () -> {
            if (swapRecentViews.isEmpty()) {
                showWelcome();
            } else {
                swapRecentViews.forEach(WorkbenchPart::initialize);
                if (null != swapRecentViewSelected.value)
                    workbench.selectMainView(swapRecentViewSelected.value.id().get());
            }
        }));
    }

    private Preferences createRecentViews(boolean load) {
        return new PreferencesInProperties(UserPrefs.confDir().resolve(".recentviews"), load);
    }

    private void saveRecentViews() {
        final Preferences recents = createRecentViews(false);
        workbench.mainViews.getTabs().forEach(tab -> {
            if (tab.getUserData() instanceof ItemRenderer itemView && itemView instanceof RecentViewSupport) {
                recents.setProperty(itemView.item().getPath(),
                        String.valueOf(itemView.item().provider.providerId())
                                .concat("|").concat(String.valueOf(tab.isSelected()))
                                .concat("|").concat(itemView.item().getName())
                );
            }
        });
        recents.save();
    }

    private void showWelcome() {
        Path indexHtml = DesktopApp.appDir().resolve("template/index.html");
        ItemEx indexItem = new ItemEx("欢迎使用", indexHtml.toString(), FileProvider.ONE);
        HtmlBasedViewer newViewer = new HtmlBasedViewer(workbench, null, indexItem);
        // 优先查找存在的视图，以避免重复打开
        HtmlBasedViewer oldViewer = (HtmlBasedViewer) workbench.findMainViewPart(newViewer.id.get());

        FxHelper.runLater(() -> {
            if (null != oldViewer) {
                workbench.selectMainView(oldViewer.id.get());
                return;
            }

            workbench.addWorkbenchPartAsMainView(newViewer, false);
            newViewer.initialize();
            workbench.selectMainView(newViewer.id.get());
        });
    }
}
