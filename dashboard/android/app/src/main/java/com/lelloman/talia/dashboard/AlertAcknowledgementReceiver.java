package com.lelloman.talia.dashboard;
import android.app.*;
import android.content.*;
import org.json.*;
public final class AlertAcknowledgementReceiver extends BroadcastReceiver {
 @Override public void onReceive(Context context,Intent intent){PendingResult pending=goAsync();new Thread(()->{String key=intent.getStringExtra("key");try{
  AlertApi.request(context,intent.getIntExtra("port",18744),"acknowledge",new JSONObject().put("key",key).put("occurrence",intent.getLongExtra("occurrence",0)).put("expected",intent.getLongExtra("revision",0)));
  context.getSystemService(NotificationManager.class).cancel(key,0);
 }catch(Exception e){Intent open=new Intent(context,AlertsActivity.class).putExtra("alertKey",key).putExtra("port",intent.getIntExtra("port",18744));PendingIntent content=PendingIntent.getActivity(context,1,open,PendingIntent.FLAG_UPDATE_CURRENT|PendingIntent.FLAG_IMMUTABLE);
  Notification n=new Notification.Builder(context,AlertPush.CHANNEL).setSmallIcon(R.drawable.ic_brand).setContentTitle("Acknowledgement failed").setContentText("Open Talìa to check the current alert and retry").setContentIntent(content).setAutoCancel(true).build();
  try{context.getSystemService(NotificationManager.class).notify(key,0,n);}catch(SecurityException ignored){}
 }finally{pending.finish();}},"talia-alert-ack").start();}
}
