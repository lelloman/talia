package com.lelloman.talia.dashboard;
public final class TaliaApplication extends android.app.Application {
 @Override public void onCreate(){super.onCreate();AlertPush.initialize(this);}
}
