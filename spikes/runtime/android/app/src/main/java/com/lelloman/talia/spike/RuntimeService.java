package com.lelloman.talia.spike;

import android.app.Service;
import android.content.Intent;
import android.os.*;

// Fixed test operations, not the production IPC/API. Services are not exported.
public class RuntimeService extends Service {
    static { System.loadLibrary("talia_runtime_spike"); }
    private static native int step(int action);
    public static final int INIT=0, GRANT=1, PING=2, EVENT=3, DIRTY=4, ABORT=5, HANG=6, SUITE=7, BASELINE=8;
    public static final int RESULT=1, STARTED=2;
    private HandlerThread thread;
    private Messenger messenger;
    @Override public void onCreate() {
        super.onCreate();
        thread = new HandlerThread("talia-runtime"); thread.start();
        messenger = new Messenger(new Handler(thread.getLooper(), message -> {
            int action = message.what;
            if (action < INIT || action > BASELINE || message.replyTo == null) return true;
            if (action == HANG) reply(message, STARTED, 0);
            int result = step(action);
            reply(message, RESULT, result);
            return true;
        }));
    }
    private void reply(Message request, int kind, int value) {
        Message response = Message.obtain(null, kind);
        Bundle data = new Bundle();
        data.putLong("token", request.getData().getLong("token"));
        data.putInt("value", value); data.putInt("pid", android.os.Process.myPid());
        response.setData(data);
        try { request.replyTo.send(response); } catch (RemoteException ignored) { }
    }
    @Override public IBinder onBind(Intent intent) { return messenger.getBinder(); }
    @Override public void onDestroy() {
        thread.quitSafely();
        super.onDestroy();
        // The parent owns process retirement. Killing here can kill a new binding
        // that arrives in this process while destruction of the old service drains.
    }
    public static class A extends RuntimeService { }
    public static class B extends RuntimeService { }
}
