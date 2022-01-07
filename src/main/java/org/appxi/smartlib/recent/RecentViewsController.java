package org.appxi.smartlib.recent;

import org.appxi.holder.RawHolder;
import org.appxi.javafx.app.AppEvent;
import org.appxi.javafx.helper.FxHelper;
import org.appxi.javafx.visual.MaterialIcon;
import org.appxi.javafx.workbench.WorkbenchPane;
import org.appxi.javafx.workbench.WorkbenchViewController;
import org.appxi.javafx.workbench.views.WorkbenchMainViewController;
import org.appxi.prefs.Preferences;
import org.appxi.prefs.PreferencesInProperties;
import org.appxi.prefs.UserPrefs;
import org.appxi.smartlib.dao.DataApi;
import org.appxi.smartlib.item.Item;
import org.appxi.smartlib.item.ItemProviders;
import org.appxi.smartlib.item.ItemViewer;

import java.util.ArrayList;
import java.util.List;
import java.util.Objects;

public class RecentViewsController extends WorkbenchViewController {
    public RecentViewsController(WorkbenchPane workbench) {
        super("recentViews", workbench);
        this.setTitles("RecentViews");
        this.viewGraphic.set(MaterialIcon.HISTORY.graphic());
    }

    @Override
    public <T> T getViewport() {
        return null;
    }

    @Override
    public void initialize() {
        app.eventBus.addEventHandler(AppEvent.STOPPING, event -> saveRecentViews());
        //
        final RawHolder<WorkbenchMainViewController> swapRecentViewSelected = new RawHolder<>();
        final List<WorkbenchMainViewController> swapRecentViews = new ArrayList<>();
        app.eventBus.addEventHandler(AppEvent.STARTING, event -> {
            final Preferences recents = createRecentViews(true);
            WorkbenchMainViewController addedController = null;

            for (String key : recents.getPropertyKeys()) {
                final String[] arr = recents.getString(key, "").split("\\|", 3);
                if (arr.length != 3) continue;

                if (!DataApi.dataAccess().exists(key))
                    continue;

                final String pid = "null".equals(arr[0]) ? null : arr[0];
                final Item item = new Item(arr[2], key,
                        ItemProviders.find(p -> !p.isDirectory() && Objects.equals(p.providerId(), pid)));

                if (null != item.provider.getViewer()) {
                    addedController = item.provider.getViewer().apply(item);
                    if (null == addedController) continue;
                    if ("true".equals(arr[1]))
                        swapRecentViewSelected.value = addedController;
                    swapRecentViews.add(addedController);
                }
            }
            if (!swapRecentViews.isEmpty()) {
                FxHelper.runLater(() -> {
                    for (WorkbenchMainViewController viewController : swapRecentViews) {
                        workbench.addWorkbenchViewAsMainView(viewController, true);
                    }
                });
                if (null == swapRecentViewSelected.value)
                    swapRecentViewSelected.value = addedController;
            }
        });
        app.eventBus.addEventHandler(AppEvent.STARTED, event -> new Thread(() -> {
            if (!swapRecentViews.isEmpty()) {
                swapRecentViews.forEach(WorkbenchViewController::initialize);
                if (null != swapRecentViewSelected.value)
                    FxHelper.runLater(() -> workbench.selectMainView(swapRecentViewSelected.value.viewId.get()));
            }
        }).start());
    }

    private Preferences createRecentViews(boolean load) {
        return new PreferencesInProperties(UserPrefs.confDir().resolve(".recentviews"), load);
    }

    private void saveRecentViews() {
        final Preferences recents = createRecentViews(false);
        workbench.getMainViewsTabs().forEach(tab -> {
            if (tab.getUserData() instanceof ItemViewer itemView) {
                recents.setProperty(itemView.item.getPath(),
                        String.valueOf(itemView.item.provider.providerId())
                                .concat("|").concat(String.valueOf(tab.isSelected()))
                                .concat("|").concat(itemView.item.getName())
                );
            }
        });
        recents.save();
    }

    @Override
    public void onViewportShowing(boolean firstTime) {
    }
}
