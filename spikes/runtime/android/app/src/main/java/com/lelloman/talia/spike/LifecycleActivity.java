package com.lelloman.talia.spike;
import android.app.Activity;
import android.content.Intent;
import android.os.*;
import android.widget.*;
import java.io.File;
import java.nio.file.Files;
public final class LifecycleActivity extends Activity {
 static {System.loadLibrary("talia_runtime_spike");}
 private static native void setFailureSignals(boolean enabled);
 private Button restart;
 private static native int step(int command,int port);
 private static native void setDashboardVisible(boolean visible);
 private HandlerThread worker; private Handler background; private final Handler main=new Handler(Looper.getMainLooper());
 private TextView status; private int port; private boolean destroyed;
 private final Runnable tick=new Runnable(){public void run(){if(destroyed)return;runStep(0);background.postDelayed(this,100);}};
 @Override public void onCreate(Bundle saved){super.onCreate(saved);createDeviceProtectedStorageContext().getFilesDir();setFailureSignals(getIntent().getBooleanExtra("failure_signals",false));port=getIntent().getIntExtra("transport_port",18743);
  LinearLayout layout=new LinearLayout(this);layout.setOrientation(LinearLayout.VERTICAL);status=new TextView(this);layout.addView(status);
  Button dirty=new Button(this);dirty.setText("Temporary edit");dirty.setOnClickListener(v->background.post(()->runStep(1)));layout.addView(dirty);
  Button action=new Button(this);action.setText("Set server value to 42");action.setOnClickListener(v->background.post(()->runStep(2)));layout.addView(action);restart=new Button(this);restart.setText("Restart dashboard");restart.setVisibility(android.view.View.GONE);restart.setOnClickListener(v->background.post(()->runStep(3)));layout.addView(restart);setContentView(layout);
  worker=new HandlerThread("talia-lifecycle");worker.start();background=new Handler(worker.getLooper());background.post(tick);
 }
 private void runStep(int command){int code=step(command,port);try{String text=code==0?new String(Files.readAllBytes(new File(createDeviceProtectedStorageContext().getFilesDir(),"lifecycle.json").toPath()),java.nio.charset.StandardCharsets.UTF_8):"Internal dashboard failure: "+code;main.post(()->{if(!destroyed){try{org.json.JSONObject report=new org.json.JSONObject(text);boolean failed=!report.isNull("failure");status.setText(failed?"Dashboard stopped — internal error\n"+report.optString("failure"):text);status.setTextColor(failed?android.graphics.Color.RED:android.graphics.Color.BLACK);restart.setVisibility(failed?android.view.View.VISIBLE:android.view.View.GONE);}catch(Exception e){status.setText(text);}}});}catch(Exception e){main.post(()->status.setText(e.toString()));}}
 @Override protected void onNewIntent(Intent intent){super.onNewIntent(intent);int command=intent.getIntExtra("command",0);background.post(()->runStep(command));}
 @Override public void onStart(){super.onStart();setDashboardVisible(true);}
 @Override public void onStop(){setDashboardVisible(false);super.onStop();}
 @Override public void onDestroy(){destroyed=true;setDashboardVisible(false);background.removeCallbacksAndMessages(null);worker.quitSafely();main.removeCallbacksAndMessages(null);super.onDestroy();}
}
