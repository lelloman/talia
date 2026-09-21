package com.lelloman.talia.dashboard;
import android.app.*;
import android.os.Bundle;
import org.json.*;
import java.util.*;
import java.nio.charset.StandardCharsets;
/** Debug APK only. Exercises the same receive path used by FirebaseMessagingService. */
public final class AlertFixtureActivity extends Activity {
 @Override public void onCreate(Bundle b){super.onCreate(b);AlertPush.prefs(this).edit().putInt("port",getIntent().getIntExtra("port",18744)).commit();String mode=getIntent().getStringExtra("mode");
  if("register".equals(mode)){AlertPush.prefs(this).edit().putString("pushToken",getIntent().getStringExtra("token")).commit();AlertPush.io.execute(()->{boolean ok=AlertPush.register(this);report(ok?"registered":"failed");runOnUiThread(this::finish);});return;}
  try{
   if("deliver".equals(mode)){JSONObject data=new JSONObject(getIntent().getStringExtra("data"));Map<String,String> map=new HashMap<>();for(Iterator<String> it=data.keys();it.hasNext();){String key=it.next();map.put(key,data.getString(key));}AlertPush.receive(this,map);}
   else if("ack".equals(mode)||"open".equals(mode)){for(android.service.notification.StatusBarNotification n:getSystemService(NotificationManager.class).getActiveNotifications()){if(n.getTag().equals(getIntent().getStringExtra("key"))){if("ack".equals(mode))n.getNotification().actions[0].actionIntent.send();else n.getNotification().contentIntent.send();break;}}}
   report("ok");
  }catch(Exception e){report("failed");}finish();
 }
 void report(String status){try{JSONArray notifications=new JSONArray();for(android.service.notification.StatusBarNotification n:getSystemService(NotificationManager.class).getActiveNotifications())notifications.put(new JSONObject().put("key",n.getTag()).put("title",n.getNotification().extras.getString(Notification.EXTRA_TITLE)).put("actions",n.getNotification().actions==null?0:n.getNotification().actions.length));JSONObject r=new JSONObject().put("status",status).put("notificationsEnabled",getSystemService(NotificationManager.class).areNotificationsEnabled()).put("pushStatus",AlertPush.prefs(this).getString("pushStatus","")).put("installation",AlertPush.installation(this)).put("deviceVersion",AlertPush.prefs(this).getLong("deviceVersion",0)).put("notifications",notifications);try(java.io.FileOutputStream out=openFileOutput("alert-fixture.json",0)){out.write(r.toString().getBytes(StandardCharsets.UTF_8));}}catch(Exception ignored){}}
}
