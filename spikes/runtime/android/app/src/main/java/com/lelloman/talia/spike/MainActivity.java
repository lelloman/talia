package com.lelloman.talia.spike;

import android.app.Activity;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.widget.LinearLayout;
import android.widget.TextView;
import java.nio.file.Files;
import java.io.File;

// Minimal embedding shell, not the production renderer or client architecture.
public class MainActivity extends Activity {
    static { System.loadLibrary("talia_runtime_spike"); }
    private static native int runNative();
    private final Handler main = new Handler(Looper.getMainLooper());
    private boolean active = true;
    private int ticks = 0;
    @Override public void onCreate(Bundle saved) {
        super.onCreate(saved);
        createDeviceProtectedStorageContext().getFilesDir();
        LinearLayout layout = new LinearLayout(this);
        layout.setOrientation(LinearLayout.VERTICAL);
        TextView heartbeat = new TextView(this), result = new TextView(this);
        layout.addView(heartbeat); layout.addView(result); setContentView(layout);
        result.setText("Running native JS fixtures…");
        Runnable pulse = new Runnable() {
            public void run() {
                if (!active) return;
                heartbeat.setText("UI heartbeat: " + (++ticks)); main.postDelayed(this, 10);
            }
        };
        main.post(pulse);
        main.postDelayed(() -> {
        final int startTicks = ticks;
        new Thread(() -> {
            int code = runNative();
            String report;
            try {
                report = code == 0 ? new String(Files.readAllBytes(new File(createDeviceProtectedStorageContext().getFilesDir(), "result.json").toPath()), java.nio.charset.StandardCharsets.UTF_8) : "FAILED: " + code;
            } catch (Exception e) { report = "FAILED: " + e; }
            final String text = report;
            main.post(() -> {
                if (!active) return;
                result.setText(text);
                android.util.Log.i("TaliaRuntimeSpike", text);
                android.util.Log.i("TaliaRuntimeSpike", "UI_TICKS=" + (ticks - startTicks));
                try { Files.write(new File(createDeviceProtectedStorageContext().getFilesDir(), "ui-ticks.txt").toPath(), Integer.toString(ticks - startTicks).getBytes(java.nio.charset.StandardCharsets.UTF_8)); }
                catch (Exception e) { result.setText("FAILED recording heartbeat: " + e); }
            });
        }, "talia-js-host").start();
        }, 500);
    }
    @Override public void onDestroy() { active = false; main.removeCallbacksAndMessages(null); super.onDestroy(); }
}
