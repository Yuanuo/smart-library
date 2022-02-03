package org.appxi.smartlib;

import org.appxi.smartcn.pinyin.Pinyin;
import org.appxi.smartcn.pinyin.PinyinConvertors;
import org.appxi.smartlib.event.GenericEvent;
import org.appxi.timeago.TimeAgo;
import org.appxi.util.StringHelper;
import org.springframework.beans.factory.BeanFactory;
import org.springframework.context.annotation.AnnotationConfigApplicationContext;
import org.springframework.core.io.Resource;
import org.springframework.core.io.UrlResource;

import java.net.URL;
import java.util.List;
import java.util.Map;

public abstract class AppContext {
    private static AnnotationConfigApplicationContext beans;
    private static final Object _initBeans = new Object();

    private static BeanFactory beans() {
        if (null != beans)
            return beans;
        synchronized (_initBeans) {
            if (null != beans)
                return beans;
            try {
                beans = new AnnotationConfigApplicationContext(SpringConfig.class) {
                    @Override
                    public Resource[] getResources(String locationPattern) {
                        if ("classpath*:org/appxi/smartlib/dao/**/*.class".equals(locationPattern)) {
                            URL url = AppContext.class.getResource("/org/appxi/smartlib/dao/PiecesRepository.class");
                            return null == url ? new Resource[0] : new Resource[]{new UrlResource(url)};
                        }
                        return new Resource[0];
                    }
                };
                App.app().eventBus.fireEvent(new GenericEvent(GenericEvent.BEANS_READY));
                App.app().logger.warn(StringHelper.concat("beans init after: ",
                        System.currentTimeMillis() - App.app().startTime));
            } catch (Throwable t) {
                t.printStackTrace();
            }
        }
        return beans;
    }

    public static <T> T getBean(Class<T> requiredType) {
        try {
            return beans().getBean(requiredType);
        } catch (Throwable ignore) {
            return null;
        }
    }

    static void setupInitialize() {
        beans();
    }

    private AppContext() {
    }

    private static TimeAgo.Messages timeAgoI18N;
    private static final Object _initTimeAgoI18N = new Object();

    public static TimeAgo.Messages timeAgoI18N() {
        if (null == timeAgoI18N)
            synchronized (_initTimeAgoI18N) {
                if (null == timeAgoI18N)
                    timeAgoI18N = TimeAgo.MessagesBuilder.start().withLocale("zh").build();
            }
        return timeAgoI18N;
    }

    public static String ascii(String text) {
        final List<Map.Entry<Character, Pinyin>> pinyinList = PinyinConvertors.convert(text);
        StringBuilder result = new StringBuilder(pinyinList.size() * (6));
        pinyinList.forEach(entry -> {
            if (null == entry.getValue()) result.append(entry.getKey());
            else result.append(" ").append(entry.getValue().getPinyinWithoutTone()).append(" ");
        });

        // 原始数据中的空格有多有少，此处需要保证仅有1个空格，以方便匹配用户输入的数据
        return result.toString().replaceAll("\s+", " ").strip();
    }

}
