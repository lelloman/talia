package com.lelloman.talia.dashboard;
public final class PushRegistrationJob extends android.app.job.JobService {
 @Override public boolean onStartJob(android.app.job.JobParameters params){AlertPush.io.execute(()->jobFinished(params,!AlertPush.register(this)));return true;}
 @Override public boolean onStopJob(android.app.job.JobParameters params){return true;}
}
