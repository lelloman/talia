package com.lelloman.talia.dashboard;
import android.app.*;
import android.app.job.*;
import android.content.*;
import android.os.Build;
import com.google.firebase.*;
import com.google.firebase.messaging.FirebaseMessaging;
import org.json.*;
import java.util.*;
import java.util.concurrent.*;
final class AlertPush {
 static final ExecutorService io=Executors.newSingleThreadExecutor();static final String CHANNEL="talia-alerts";
 static android.content.SharedPreferences prefs(Context c){return c.getSharedPreferences("talia-alerts",0);}
 static synchronized String installation(Context c){String id=prefs(c).getString("installation",null);if(id==null){id=UUID.randomUUID().toString();prefs(c).edit().putString("installation",id).commit();}return id;}
 static void initialize(Context context){
  Context c=context.getApplicationContext();installation(c);NotificationManager n=c.getSystemService(NotificationManager.class);n.createNotificationChannel(new NotificationChannel(CHANNEL,"Monitoring alerts",NotificationManager.IMPORTANCE_HIGH));
  if(BuildConfig.FIREBASE_APP_ID.isEmpty()){prefs(c).edit().putString("pushStatus","Push provider is not configured").apply();return;}
  try{if(FirebaseApp.getApps(c).isEmpty())FirebaseApp.initializeApp(c,new FirebaseOptions.Builder().setApplicationId(BuildConfig.FIREBASE_APP_ID).setApiKey(BuildConfig.FIREBASE_API_KEY).setProjectId(BuildConfig.FIREBASE_PROJECT_ID).setGcmSenderId(BuildConfig.FIREBASE_SENDER_ID).build());
   FirebaseMessaging.getInstance().getToken().addOnSuccessListener(token->token(c,token)).addOnFailureListener(e->prefs(c).edit().putString("pushStatus","Push token unavailable").apply());
  }catch(Exception e){prefs(c).edit().putString("pushStatus","Invalid push configuration").apply();}
 }
 static void token(Context c,String token){prefs(c).edit().putString("pushToken",token).apply();schedule(c);}
 static void schedule(Context c){if(prefs(c).getString("credential","").isEmpty()||prefs(c).getString("pushToken","").isEmpty())return;
  c.getSystemService(JobScheduler.class).schedule(new JobInfo.Builder(701,new ComponentName(c,PushRegistrationJob.class)).setRequiredNetworkType(JobInfo.NETWORK_TYPE_ANY).setPersisted(true).setBackoffCriteria(30000,JobInfo.BACKOFF_POLICY_EXPONENTIAL).build());
 }
 static boolean register(Context c){try{
  String id=installation(c),token=prefs(c).getString("pushToken","");if(token.isEmpty())return true;int port=prefs(c).getInt("port",18744);
  JSONObject state=AlertApi.request(c,port,"device_status",new JSONObject().put("id",id));JSONObject device=state.optJSONObject("device");long version=device==null?0:device.getLong("version");if(device!=null&&!device.getBoolean("enabled")){prefs(c).edit().putString("pushStatus","Push device disabled").apply();return true;}
  JSONObject request=new JSONObject().put("expected",version).put("device",new JSONObject().put("id",id).put("version",version+1).put("owner","").put("token",token).put("enabled",true));
  AlertApi.request(c,port,"device_register",request);prefs(c).edit().putLong("deviceVersion",version+1).putString("pushStatus","Push registered").apply();return true;
 }catch(Exception e){prefs(c).edit().putString("pushStatus","Push registration failed; will retry").apply();return false;}}
 static void receive(Context c,Map<String,String> data){try{
  String key=data.get("key"),message=data.get("message"),severity=data.get("severity");long occurrence=Long.parseLong(data.get("occurrence")),revision=Long.parseLong(data.get("revision")),expires=Long.parseLong(data.get("expires"));
  if(key==null||key.isEmpty()||key.length()>256||message==null||message.length()>4096||occurrence<=0||revision<=0||expires<=System.currentTimeMillis())return;
  if(Build.VERSION.SDK_INT>=33&&c.checkSelfPermission(android.Manifest.permission.POST_NOTIFICATIONS)!=android.content.pm.PackageManager.PERMISSION_GRANTED)return;
  int port=prefs(c).getInt("port",18744);Intent open=new Intent(c,AlertsActivity.class).putExtra("port",port).putExtra("alertKey",key).setData(android.net.Uri.parse("talia://alert/"+android.net.Uri.encode(key))).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK|Intent.FLAG_ACTIVITY_CLEAR_TOP);
  PendingIntent content=PendingIntent.getActivity(c,0,open,PendingIntent.FLAG_UPDATE_CURRENT|PendingIntent.FLAG_IMMUTABLE);
  Notification.Builder notification=new Notification.Builder(c,CHANNEL).setSmallIcon(R.drawable.ic_brand).setContentTitle("Talìa · "+severity+("false".equals(data.get("active"))?" · Resolved":"")).setContentText(message).setStyle(new Notification.BigTextStyle().bigText(message)).setContentIntent(content).setAutoCancel(true).setTimeoutAfter(expires-System.currentTimeMillis());
  if(!"false".equals(data.get("active"))){Intent ack=new Intent(c,AlertAcknowledgementReceiver.class).putExtra("key",key).putExtra("occurrence",occurrence).putExtra("revision",revision).putExtra("port",port).setData(android.net.Uri.parse("talia://ack/"+android.net.Uri.encode(key)+"/"+occurrence+"/"+revision));notification.addAction(new Notification.Action.Builder(null,"Acknowledge",PendingIntent.getBroadcast(c,0,ack,PendingIntent.FLAG_UPDATE_CURRENT|PendingIntent.FLAG_IMMUTABLE)).build());}
  c.getSystemService(NotificationManager.class).notify(key,0,notification.build());
 }catch(Exception ignored){prefs(c).edit().putString("pushStatus","Invalid push message ignored").apply();}}
}
