package com.lelloman.talia.dashboard;
import android.app.Activity;
import android.os.*;
import android.widget.*;
import android.view.*;
import org.json.*;
import java.io.*;
import java.net.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.*;
import java.util.concurrent.*;

/** P1 loopback client. The native VM is confined to a HandlerThread; I/O is async. */
public final class MainActivity extends Activity {
 static{System.loadLibrary("talia_dashboard_runtime");}
 private static native String evaluate(String source,boolean ui,boolean reset);
 final Handler main=new Handler(Looper.getMainLooper());HandlerThread thread;Handler worker;
 final ExecutorService io=Executors.newFixedThreadPool(4);
 final Map<String,Long> subscriptions=new LinkedHashMap<>();final Map<String,JSONObject> actions=new LinkedHashMap<>();
 NativeRenderer renderer;LinearLayout root,body;TextView status,update,dirty;Button restart;
 volatile boolean visible,destroyed,sidebar;boolean started,failed,polling;long epoch=1,next=0,lastRequest=0;int port;volatile int width=1000;String externalError=null;JSONObject loaded,pendingPackage;boolean checking,updateAvailable;long lastUpdateCheck=0;
 final Runnable tick=new Runnable(){public void run(){if(destroyed)return;if(started&&visible&&!failed){try{refresh();poll();checkUpdates();}catch(Exception e){fail(e);}}worker.postDelayed(this,150);}};
 @Override public void onCreate(Bundle state){super.onCreate(state);port=getIntent().getIntExtra("port",18744);
  root=new LinearLayout(this);root.setOrientation(1);root.setPadding(16,16,16,16);root.setBackgroundColor(0xfff5f8ff);
  if(Build.VERSION.SDK_INT>=30)root.setOnApplyWindowInsetsListener((v,insets)->{android.graphics.Insets i=insets.getInsets(WindowInsets.Type.systemBars());v.setPadding(i.left+16,i.top+16,i.right+16,i.bottom+16);return insets;});
  ImageView brand=new ImageView(this);brand.setImageResource(com.lelloman.talia.dashboard.R.drawable.ic_brand);brand.setContentDescription("Talìa");root.addView(brand,new LinearLayout.LayoutParams(80,80));
  sidebar=getPreferences(0).getBoolean("sidebar",false);CheckBox composition=new CheckBox(this);composition.setText("Side navigation");composition.setChecked(sidebar);root.addView(composition);composition.setOnCheckedChangeListener((button,checked)->{sidebar=checked;getPreferences(0).edit().putBoolean("sidebar",checked).apply();});
  status=new TextView(this);status.setAccessibilityLiveRegion(View.ACCESSIBILITY_LIVE_REGION_ASSERTIVE);status.setVisibility(View.GONE);root.addView(status);
  update=new TextView(this);update.setText("Update available");update.setVisibility(View.GONE);update.setAccessibilityLiveRegion(View.ACCESSIBILITY_LIVE_REGION_POLITE);root.addView(update);
  dirty=new TextView(this);dirty.setText("Temporary ViewModel changes · reload to discard");dirty.setVisibility(View.GONE);root.addView(dirty);
  Button reload=new Button(this);reload.setText("Reload dashboard");reload.setAllCaps(false);reload.setOnClickListener(v->worker.post(this::start));root.addView(reload);
  restart=new Button(this);restart.setText("Restart dashboard");restart.setAllCaps(false);restart.setVisibility(View.GONE);root.addView(restart);
  body=new LinearLayout(this);body.setOrientation(1);root.addView(body,new LinearLayout.LayoutParams(-1,0,1));setContentView(root);
  renderer=new NativeRenderer(this,body,(action,target,value)->worker.post(()->{if(!visible||failed)return;try{JSONObject event=new JSONObject().put("target",target).put("value",value);eval("TaliaVM.dispatch("+JSONObject.quote(action)+","+event+");'ok';",false,false);refresh();}catch(Exception e){fail(e);}}));
  root.addOnLayoutChangeListener((v,l,t,r,b,ol,ot,or,ob)->width=body.getWidth());
  thread=new HandlerThread("talia-vm");thread.start();worker=new Handler(thread.getLooper());restart.setOnClickListener(v->worker.post(this::start));worker.post(this::start);worker.post(tick);
 }
 String asset(String name)throws IOException{try(InputStream in=getAssets().open(name)){return new String(in.readAllBytes(),StandardCharsets.UTF_8);}}
 String eval(String source,boolean ui,boolean reset)throws Exception{
  JSONObject result=new JSONObject(evaluate(source,ui,reset));if(result.has("error"))throw new IllegalStateException(result.getString("error"));
  if(!ui){JSONArray out=result.getJSONArray("out");for(int i=0;i<out.length();i++)request(out.getJSONObject(i));}
  return result.getString("value");
 }
 interface PackageResult{void done(JSONObject pkg)throws Exception;}
 void fetchPackage(PackageResult done){
  io.execute(()->{JSONObject pkg=null;
   try{HttpURLConnection c=(HttpURLConnection)new URL("http://127.0.0.1:"+port+"/dashboard/package.json").openConnection();try{c.setConnectTimeout(5000);c.setReadTimeout(5000);try(InputStream in=c.getInputStream()){byte[] bytes=in.readNBytes(262145);if(bytes.length>262144)throw new IOException("package size limit");pkg=new JSONObject(new String(bytes,StandardCharsets.UTF_8));}}finally{c.disconnect();}}catch(Exception e){externalError=e.toString();}
   JSONObject result=pkg;worker.post(()->{if(!destroyed)try{done.done(result);}catch(Exception e){fail(e);}});
  });
 }
 void start(){
  long stamp=++epoch;started=false;failed=false;subscriptions.clear();lastRequest=0;polling=false;
  fetchPackage(pkg->{if(stamp!=epoch)return;if(pkg==null){File cache=new File(getFilesDir(),"saved.json");pkg=new JSONObject(cache.exists()?new String(Files.readAllBytes(cache.toPath()),StandardCharsets.UTF_8):asset("monitor.json"));}
   if(!visible){pendingPackage=pkg;return;}installPackage(pkg);
  });
 }
 void installPackage(JSONObject pkg)throws Exception{
  loaded=pkg;pendingPackage=null;updateAvailable=false;
  eval(asset("ui.js")+"\nglobalThis.savedPackage="+pkg+";TaliaUI.validatePackage(savedPackage);'ok';",true,true);
  eval(asset("vm.js")+"\n"+pkg.getString("viewModel")+"\nTaliaVM.start("+pkg.optJSONObject("params")+");'ok';",false,true);started=true;
  Files.write(new File(getFilesDir(),"saved.json").toPath(),pkg.toString().getBytes(StandardCharsets.UTF_8));
  main.post(()->{status.setVisibility(View.GONE);restart.setVisibility(View.GONE);update.setVisibility(View.GONE);body.setEnabled(true);body.setVisibility(View.VISIBLE);});refresh();
 }
 void checkUpdates(){
  if(checking||loaded==null||SystemClock.elapsedRealtime()-lastUpdateCheck<2000)return;checking=true;lastUpdateCheck=SystemClock.elapsedRealtime();long stamp=epoch;
  fetchPackage(pkg->{checking=false;if(pkg!=null&&stamp==epoch&&visible){boolean changed=!pkg.getString("revision").equals(loaded.getString("revision"));updateAvailable=changed;main.post(()->update.setVisibility(changed?View.VISIBLE:View.GONE));}});
 }
 void refresh()throws Exception{
  JSONObject report=new JSONObject(eval("JSON.stringify(TaliaVM.snapshot())",false,false));if(!report.isNull("failure")){fail(new IllegalStateException(report.getString("failure")));return;}
  String resolved=eval("JSON.stringify(TaliaUI.resolve(savedPackage.ui,"+report.getJSONObject("state")+",{definitions:savedPackage.definitions,width:"+Math.max(0,width)+",scale:"+getResources().getDisplayMetrics().density+",params:{...savedPackage.params,sidebar:"+sidebar+"}}))",true,false);
  JSONObject node=new JSONObject(resolved);report.put("definitionRevision",loaded.getString("revision"));report.put("updateAvailable",updateAvailable);boolean isDirty=report.getBoolean("dirty");report.put("width",width);report.put("scale",getResources().getDisplayMetrics().density);report.put("sidebar",sidebar);report.put("subscriptions",subscriptions.size());report.put("actions",new JSONArray(actions.values()));report.put("externalError",externalError==null?JSONObject.NULL:externalError);
  Files.write(new File(getFilesDir(),"report.json").toPath(),report.toString().getBytes(StandardCharsets.UTF_8));
  main.post(()->{if(destroyed||failed)return;try{dirty.setVisibility(isDirty?View.VISIBLE:View.GONE);renderer.render(node);}catch(Exception e){worker.post(()->fail(e));}});
 }
 void fail(Exception error){if(failed)return;failed=true;epoch++;subscriptions.clear();String message="Dashboard stopped — "+error;
  try{Files.write(new File(getFilesDir(),"report.json").toPath(),new JSONObject().put("failure",message).toString().getBytes(StandardCharsets.UTF_8));}catch(Exception ignored){}
  main.post(()->{status.setText(message);status.setTextColor(0xff991b1b);status.setVisibility(View.VISIBLE);restart.setVisibility(View.VISIBLE);body.setVisibility(View.GONE);});
 }
 interface Completion{void done(JSONObject value,String error)throws Exception;}
 void rpc(String op,JSONObject args,Completion done)throws JSONException{
  long stamp=epoch;JSONObject payload=new JSONObject().put("session","p1-dashboard").put("channel","android").put("epoch",epoch).put("id",++next).put("op",op).put("args",args);
  io.execute(()->{JSONObject value=null;String error=null;
   try{HttpURLConnection c=(HttpURLConnection)new URL("http://127.0.0.1:"+port+"/rpc").openConnection();try{c.setRequestMethod("POST");c.setDoOutput(true);c.setConnectTimeout(5000);c.setReadTimeout(5000);c.setRequestProperty("Content-Type","application/json");try(OutputStream out=c.getOutputStream()){out.write(payload.toString().getBytes(StandardCharsets.UTF_8));}try(InputStream in=c.getInputStream()){JSONObject reply=new JSONObject(new String(in.readAllBytes(),StandardCharsets.UTF_8));if(reply.has("error"))error=reply.getString("error");else value=reply.getJSONObject("value");}}finally{c.disconnect();}}
   catch(Exception e){error=e.toString();}JSONObject result=value;String problem=error;
   worker.post(()->{if(stamp!=epoch||destroyed||!visible||failed)return;try{done.done(result,problem);}catch(Exception e){fail(e);}});
  });
 }
 void deliver(JSONObject message)throws Exception{if(visible&&!failed)eval("TaliaVM.receive("+JSONObject.quote(message.toString())+");'ok';",false,false);}
 void request(JSONObject r)throws Exception{
  if(!visible||failed)return;long id=r.getLong("id");if(id<=lastRequest)throw new IllegalStateException("bridge replay");lastRequest=id;String op=r.getString("op");Object value=r.get("value");
  switch(op){
   case "subscribe":if(!value.equals("value")||subscriptions.size()>=16)throw new IllegalStateException("subscription grant/budget");String sid="s"+id;subscriptions.put(sid,-1L);deliver(new JSONObject().put("id",id).put("value",sid));break;
   case "unsubscribe":if(subscriptions.remove(value.toString())==null)throw new IllegalStateException("subscription ownership");deliver(new JSONObject().put("id",id).put("value",JSONObject.NULL));break;
   case "read":if(!value.equals("value"))throw new IllegalStateException("resource grant");rpc("read",new JSONObject().put("tag","android-read"),(v,e)->deliver(new JSONObject().put("id",id).put(e==null?"value":"error",e==null?v:e)));break;
   case "write":{
    if(!(value instanceof Number)||((Number)value).doubleValue()!=((Number)value).longValue()||Math.abs(((Number)value).doubleValue())>1000000)throw new IllegalStateException("write value");
    String action="a-"+UUID.randomUUID();actions.put(action,new JSONObject().put("status","unknown"));rpc("action",new JSONObject().put("actionId",action).put("value",value),(v,e)->{if(e==null)actions.put(action,v);deliver(new JSONObject().put("id",id).put(e==null?"value":"error",e==null?v:e));});break;
   }
   default:throw new IllegalStateException("operation not granted");
  }
 }
 void poll()throws Exception{if(polling||subscriptions.isEmpty())return;polling=true;
  rpc("read",new JSONObject().put("tag","android-subscription"),(v,e)->{polling=false;if(e!=null){externalError=e;return;}for(String id:new ArrayList<>(subscriptions.keySet()))if(v.getLong("revision")>subscriptions.get(id)){subscriptions.put(id,v.getLong("revision"));deliver(new JSONObject().put("event",id).put("value",v));}});
 }
 @Override public void onStart(){super.onStart();visible=true;if(worker!=null)worker.post(()->{if(failed)return;try{if(pendingPackage!=null){installPackage(pendingPackage);return;}if(!started){start();return;}epoch++;polling=false;eval("TaliaVM.resume();'ok';",false,false);subscriptions.replaceAll((k,v)->-1L);for(String id:new ArrayList<>(actions.keySet()))rpc("status",new JSONObject().put("actionId",id),(v,e)->{if(e==null)actions.put(id,v);});refresh();poll();}catch(Exception e){fail(e);}});}
 @Override public void onStop(){visible=false;if(worker!=null)worker.post(()->{epoch++;polling=false;if(started&&!failed)try{eval("TaliaVM.pause();'ok';",false,false);}catch(Exception e){fail(e);}});super.onStop();}
 @Override protected void onNewIntent(android.content.Intent intent){super.onNewIntent(intent);
  if(BuildConfig.DEBUG&&intent.hasExtra("live"))worker.post(()->{if(!started||!visible||failed)return;try{eval("TaliaVM.markDirty();\n"+intent.getStringExtra("live")+"\n;'ok';",false,false);refresh();}catch(Exception e){fail(e);}});
 }
 @Override public void onDestroy(){destroyed=true;io.shutdownNow();worker.removeCallbacksAndMessages(null);thread.quitSafely();main.removeCallbacksAndMessages(null);super.onDestroy();}
}
