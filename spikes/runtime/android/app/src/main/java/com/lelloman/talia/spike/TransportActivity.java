package com.lelloman.talia.spike;

import android.app.Activity;
import android.os.*;
import android.widget.TextView;
import java.nio.file.Files;
import java.io.File;

// Explicitly launched experiment; no production dashboard UI or background service.
public class TransportActivity extends Activity {
    static { System.loadLibrary("talia_runtime_spike"); }
    private static native int runTransport(int port);
    private final Handler main = new Handler(Looper.getMainLooper());
    private int ticks;
    @Override public void onCreate(Bundle saved) {
        super.onCreate(saved);
        createDeviceProtectedStorageContext().getFilesDir();
        TextView view=new TextView(this);setContentView(view);
        Runnable pulse=new Runnable(){public void run(){view.setText("Transport experiment: "+(++ticks));main.postDelayed(this,10);}};
        main.post(pulse);
        int port=getIntent().getIntExtra("transport_port",0);
        if(port<1 || port>65535){view.setText("Missing test server port");return;}
        new Thread(()->{
            int code=runTransport(port);
            String result;
            try {
                result=code==0 ? new String(Files.readAllBytes(new File(createDeviceProtectedStorageContext().getFilesDir(),"transport.json").toPath()),java.nio.charset.StandardCharsets.UTF_8) : "FAILED: "+code;
            }catch(Exception error){result="FAILED: "+error;}
            final String text=result;
            main.post(()->{
                main.removeCallbacks(pulse);view.setText(text);
                android.util.Log.i("TaliaTransport",text);
                android.util.Log.i("TaliaTransport","UI_TICKS="+ticks);
            });
        },"talia-transport-test").start();
    }
    @Override public void onDestroy(){main.removeCallbacksAndMessages(null);super.onDestroy();}
}
