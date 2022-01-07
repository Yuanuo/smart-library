package org.appxi.smartlib.dao;

import java.nio.file.Path;

public abstract class DataApi {
    private DataApi() {
    }

    private static DataAccess dataAccess;

    public static DataAccess dataAccess() {
        return dataAccess;
    }

    public static void setupInitialize(Path repository) {
        dataAccess = null != dataAccess ? dataAccess : new DataAccessImpl(repository);
    }
}
