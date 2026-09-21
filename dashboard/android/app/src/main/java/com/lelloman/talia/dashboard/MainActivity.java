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
 private static native void retire();
 private static native String evaluate(String source,boolean ui,boolean reset);
 final Handler main=new Handler(Looper.getMainLooper());HandlerThread thread;Handler worker;
 final ExecutorService io=new ThreadPoolExecutor(4,4,0L,TimeUnit.MILLISECONDS,new ArrayBlockingQueue<>(64));
 final Map<String,String> subscriptionResources=new LinkedHashMap<>(),sampleKeys=new LinkedHashMap<>();
 final Map<String,Long> subscriptions=new LinkedHashMap<>();final Map<String,JSONObject> actions=new LinkedHashMap<>();
 NativeRenderer renderer;LinearLayout root,body;TextView status,update,dirty,connectionStatus;DurableConnection connection;ClientRegistry registry;long editRevision=0,assignmentRevision=0,loadRequest=0;boolean cachedBaseline,loadingSaved;String requestedDashboard;JSONObject selectedDelivery;boolean registryDirty;boolean durable;String lastSampleKey="";Button restart;
 volatile boolean visible,destroyed,sidebar;boolean started,polling;volatile boolean failed;volatile long epoch=1;long next=0,lastRequest=0;int port;volatile int width=1000;String externalError=null;String dashboardId="monitor";String nextActionId="a-"+UUID.randomUUID();final JSONArray failureSignals=new JSONArray();JSONObject loaded,pendingPackage;boolean checking,updateAvailable;long lastUpdateCheck=0;
 final Runnable tick=new Runnable(){public void run(){if(destroyed)return;if(connection!=null)connection.tick();reportRegistry(false);if(started&&visible&&!failed){try{refresh();poll();checkUpdates();}catch(Exception e){fail(e);}}worker.postDelayed(this,150);}};
 @Override public void onCreate(Bundle state){super.onCreate(state);durable=getIntent().getBooleanExtra("durable",getPreferences(0).getBoolean("durable",false));getPreferences(0).edit().putBoolean("durable",durable).apply();port=getIntent().getIntExtra("port",getPreferences(0).getInt("port",18744));getPreferences(0).edit().putInt("port",port).apply();requestedDashboard=getIntent().getStringExtra("dashboard");dashboardId=requestedDashboard;if(dashboardId==null)dashboardId=getPreferences(0).getString("dashboard","monitor");if(!dashboardId.matches("[A-Za-z][A-Za-z0-9_-]{0,63}"))throw new IllegalArgumentException("dashboard ID");getPreferences(0).edit().putString("dashboard",dashboardId).apply();
  root=new LinearLayout(this);root.setOrientation(1);root.setPadding(16,16,16,16);root.setBackgroundColor(0xfff5f8ff);
  if(Build.VERSION.SDK_INT>=30)root.setOnApplyWindowInsetsListener((v,insets)->{android.graphics.Insets i=insets.getInsets(WindowInsets.Type.systemBars());v.setPadding(i.left+16,i.top+16,i.right+16,i.bottom+16);return insets;});
  ImageView brand=new ImageView(this);brand.setImageResource(com.lelloman.talia.dashboard.R.drawable.ic_brand);brand.setContentDescription("Talìa");root.addView(brand,new LinearLayout.LayoutParams(80,80));
  sidebar=getPreferences(0).getBoolean("sidebar",false);CheckBox composition=new CheckBox(this);composition.setText("Side navigation");composition.setChecked(sidebar);root.addView(composition);composition.setOnCheckedChangeListener((button,checked)->{sidebar=checked;getPreferences(0).edit().putBoolean("sidebar",checked).apply();});
  connectionStatus=new TextView(this);connectionStatus.setAccessibilityLiveRegion(View.ACCESSIBILITY_LIVE_REGION_POLITE);root.addView(connectionStatus);
  status=new TextView(this);status.setAccessibilityLiveRegion(View.ACCESSIBILITY_LIVE_REGION_ASSERTIVE);status.setVisibility(View.GONE);root.addView(status);
  update=new TextView(this);update.setText("Update available");update.setVisibility(View.GONE);update.setAccessibilityLiveRegion(View.ACCESSIBILITY_LIVE_REGION_POLITE);root.addView(update);
  dirty=new TextView(this);dirty.setText("Temporary ViewModel changes · reload to discard");dirty.setVisibility(View.GONE);root.addView(dirty);
  Button reload=new Button(this);reload.setText("Reload dashboard");reload.setAllCaps(false);reload.setOnClickListener(v->worker.post(this::start));root.addView(reload);
  restart=new Button(this);restart.setText("Restart dashboard");restart.setAllCaps(false);restart.setVisibility(View.GONE);root.addView(restart);
  body=new LinearLayout(this);body.setOrientation(1);root.addView(body,new LinearLayout.LayoutParams(-1,0,1));setContentView(root);
  renderer=new NativeRenderer(this,body,(action,target,value)->worker.post(()->{if(!visible||failed)return;try{JSONObject event=new JSONObject().put("target",target).put("value",value);eval("TaliaVM.dispatch("+JSONObject.quote(action)+","+event+");'ok';",false,false);refresh();}catch(Exception e){fail(e);}}));
  root.addOnLayoutChangeListener((v,l,t,r,b,ol,ot,or,ob)->width=body.getWidth());
  thread=new HandlerThread("talia-vm");thread.start();worker=new Handler(thread.getLooper());if(durable)registry=new ClientRegistry(getSharedPreferences("talia-registration-"+port,0),worker,io,port);if(durable)connection=new DurableConnection(worker,io,port,actions,text->main.post(()->{connectionStatus.setText(text);connectionStatus.setVisibility(text.isEmpty()?View.GONE:View.VISIBLE);}),v->{if(started&&visible&&!failed)try{durableSample(v);}catch(Exception e){fail(e);}});else connectionStatus.setVisibility(View.GONE);restart.setOnClickListener(v->worker.post(this::start));worker.post(this::start);worker.post(tick);
 }
 static byte[] readLimited(InputStream in,int max)throws IOException{ByteArrayOutputStream out=new ByteArrayOutputStream();byte[] buffer=new byte[4096];for(int n;(n=in.read(buffer))!=-1;){if(out.size()+n>max)throw new IOException("response size limit");out.write(buffer,0,n);}return out.toByteArray();}
 String asset(String name)throws IOException{try(InputStream in=getAssets().open(name)){return new String(readLimited(in,262144),StandardCharsets.UTF_8);}}
 String eval(String source,boolean ui,boolean reset)throws Exception{
  JSONObject result=new JSONObject(evaluate(source,ui,reset));if(result.has("error"))throw new IllegalStateException(result.getString("error"));
  if(!ui){JSONArray out=result.getJSONArray("out");for(int i=0;i<out.length();i++)request(out.getJSONObject(i));}
  return result.getString("value");
 }
 interface PackageResult{void done(JSONObject pkg)throws Exception;}
 void fetchPackage(PackageResult done){
  io.execute(()->{JSONObject pkg=null;
   try{HttpURLConnection c=(HttpURLConnection)new URL("http://127.0.0.1:"+port+"/dashboard/package.json?dashboard="+dashboardId).openConnection();try{c.setConnectTimeout(5000);c.setReadTimeout(5000);try(InputStream in=c.getInputStream()){byte[] bytes=readLimited(in,262144);if(bytes.length>262144)throw new IOException("package size limit");pkg=new JSONObject(new String(bytes,StandardCharsets.UTF_8));}}finally{c.disconnect();}}catch(Exception e){externalError=e.toString();}
   JSONObject result=pkg;worker.post(()->{if(!destroyed)try{done.done(result);}catch(Exception e){fail(e);}});
  });
 }
 void start(){
  if(durable){startSaved();return;}
  long stamp=++epoch;started=false;failed=false;subscriptions.clear();subscriptionResources.clear();sampleKeys.clear();lastRequest=0;polling=false;
  fetchPackage(pkg->{if(stamp!=epoch)return;if(pkg==null){File cache=new File(getFilesDir(),"saved-"+dashboardId+".json");pkg=new JSONObject(cache.exists()?new String(Files.readAllBytes(cache.toPath()),StandardCharsets.UTF_8):asset("monitor.json"));}
   if(!visible){pendingPackage=pkg;return;}installPackage(pkg);
  });
 }
 void startSaved(){
  if(loadingSaved)return;loadingSaved=true;
  final long request=++loadRequest;final String selection=requestedDashboard;
  io.execute(()->{JSONObject delivery=null;Exception problem=null;
   try{if(selection!=null)registry.select(selection,new JSONObject(getIntent().getStringExtra("dashboard_params")==null?"{}":getIntent().getStringExtra("dashboard_params")));delivery=registry.prepare();delivery.put("cached",false);}
   catch(Exception e){problem=e;if(!(e instanceof ClientRegistry.ServerError)&&selection==null){try{File cache=new File(getFilesDir(),registry.cacheName());if(cache.exists()){delivery=new JSONObject(new String(Files.readAllBytes(cache.toPath()),StandardCharsets.UTF_8));delivery.put("cached",true);problem=null;}}catch(Exception ignored){}}}
   JSONObject result=delivery;Exception failure=problem;worker.post(()->{loadingSaved=false;if(destroyed||request!=loadRequest)return;if(failure!=null||result==null){loadFailed(failure==null?new IOException("No saved dashboard"):failure);return;}if(!visible)return;
    try{JSONObject pkg=result.getJSONObject("package");if(!pkg.getString("id").equals(result.getJSONObject("assignment").getString("dashboardId")))throw new IOException("dashboard assignment mismatch");
     // Validate before retiring the existing runtime; this context contains no guest state.
     if(loaded!=null&&!failed)eval("TaliaUI.validatePackage("+pkg+");'ok';",true,false);
     if(result.optBoolean("cached")){installSaved(result);return;}
     io.execute(()->{Exception rejected=null;try{registry.confirm(result.getJSONObject("assignment").getLong("revision"));}catch(Exception e){rejected=e;}Exception err=rejected;worker.post(()->{if(destroyed||request!=loadRequest||!visible)return;if(err!=null){loadFailed(err);return;}try{installSaved(result);}catch(Exception e){fail(e);}});});
    }catch(Exception e){loadFailed(e);}
   });
  });
 }
 void loadFailed(Exception e){if(started&&!failed){main.post(()->{status.setText("Reload failed — "+e);status.setVisibility(View.VISIBLE);});}else fail(e);}
 void installSaved(JSONObject delivery)throws Exception{
  JSONObject pkg=delivery.getJSONObject("package");assignmentRevision=delivery.getJSONObject("assignment").getLong("revision");cachedBaseline=delivery.optBoolean("cached");selectedDelivery=delivery;requestedDashboard=null;dashboardId=pkg.getString("id");getPreferences(0).edit().putString("dashboard",dashboardId).apply();sidebar=pkg.getJSONObject("params").optBoolean("sidebar",false);
  epoch++;started=false;failed=false;subscriptions.clear();subscriptionResources.clear();sampleKeys.clear();lastRequest=0;polling=false;if(connection!=null){connection.resources(Collections.emptyList());connection.active(false);}
  installPackage(pkg);Files.write(new File(getFilesDir(),registry.cacheName()).toPath(),delivery.toString().getBytes(StandardCharsets.UTF_8));
 }
 void installPackage(JSONObject pkg)throws Exception{
  if(!pkg.getString("id").equals(dashboardId))throw new IllegalArgumentException("dashboard assignment mismatch");loaded=pkg;pendingPackage=null;updateAvailable=false;
  editRevision=0;registryDirty=false;if(registry!=null)registry.replace(registryReport("paused"));
  eval(asset("value.js")+"\n"+asset("ui.js")+"\nglobalThis.savedPackage="+pkg+";TaliaUI.validatePackage(savedPackage);'ok';",true,true);
  eval(asset("value.js")+"\n"+asset("vm.js")+"\n"+pkg.getString("viewModel")+"\nTaliaVM.start("+pkg.optJSONObject("params")+");'ok';",false,true);started=true;
  Files.write(new File(getFilesDir(),"saved-"+dashboardId+".json").toPath(),pkg.toString().getBytes(StandardCharsets.UTF_8));
  main.post(()->{status.setVisibility(View.GONE);restart.setVisibility(View.GONE);update.setVisibility(View.GONE);body.setEnabled(true);body.setVisibility(View.VISIBLE);});refresh();
 }
 void checkUpdates(){
  if(checking||loaded==null||SystemClock.elapsedRealtime()-lastUpdateCheck<2000)return;checking=true;lastUpdateCheck=SystemClock.elapsedRealtime();long stamp=epoch;
  if(durable){io.execute(()->{JSONObject next=null;String problem=null;try{next=registry.prepare();}catch(Exception e){problem=e instanceof ClientRegistry.ServerError?e.getMessage():null;}JSONObject result=next;String error=problem;worker.post(()->{checking=false;if(stamp!=epoch||destroyed)return;try{if(result!=null){updateAvailable=!result.getJSONObject("package").getString("revision").equals(loaded.getString("revision"))||result.getJSONObject("assignment").getLong("revision")!=assignmentRevision;}else if(error!=null)updateAvailable=true;main.post(()->update.setVisibility(updateAvailable?View.VISIBLE:View.GONE));}catch(Exception ignored){}});});return;}
  fetchPackage(pkg->{checking=false;if(pkg!=null&&stamp==epoch&&visible){boolean changed=!pkg.getString("revision").equals(loaded.getString("revision"));updateAvailable=changed;main.post(()->update.setVisibility(changed?View.VISIBLE:View.GONE));}});
 }
 void refresh()throws Exception{
  long stamp=epoch;
  JSONObject report=new JSONObject(eval("JSON.stringify({...TaliaVM.snapshot(),stateWire:TaliaValue.encode(TaliaVM.snapshot().state)})",false,false));if(!report.isNull("failure")){fail(new IllegalStateException(report.getString("failure")));return;}
  String resolved=eval("JSON.stringify(TaliaUI.resolve(savedPackage.ui,TaliaValue.decode("+report.getJSONObject("stateWire")+"),{definitions:savedPackage.definitions,width:"+Math.max(0,width)+",scale:"+getResources().getDisplayMetrics().density+",params:{...savedPackage.params,sidebar:"+sidebar+"}}))",true,false);
  JSONObject node=new JSONObject(resolved);registryDirty=report.getBoolean("dirty");reportRegistry(false);report.put("registration",registry==null?JSONObject.NULL:registry.publicStatus());report.put("assignmentRevision",assignmentRevision).put("cachedBaseline",cachedBaseline);report.put("editRevision",editRevision);report.put("connection",connection==null?JSONObject.NULL:connection.state);report.put("monitoringError",connection==null||connection.monitoringError==null?JSONObject.NULL:connection.monitoringError);report.put("dashboardId",dashboardId);report.put("definitionRevision",loaded.getString("revision"));report.put("updateAvailable",updateAvailable);boolean isDirty=report.getBoolean("dirty");report.put("width",width);report.put("scale",getResources().getDisplayMetrics().density);report.put("sidebar",sidebar);report.put("subscriptions",subscriptions.size());report.put("signals",failureSignals);report.put("actions",new JSONArray(actions.values()));report.put("actionIds",new JSONArray(actions.keySet()));report.put("nextActionId",nextActionId);report.put("externalError",externalError==null?JSONObject.NULL:externalError);
  Files.write(new File(getFilesDir(),"report.json").toPath(),report.toString().getBytes(StandardCharsets.UTF_8));
  main.post(()->{if(destroyed||failed||stamp!=epoch)return;try{dirty.setVisibility(isDirty?View.VISIBLE:View.GONE);renderer.render(node);}catch(Exception e){worker.post(()->fail(e));}});
 }
 void fail(Exception error){if(failed)return;failed=true;reportRegistry(true);epoch++;subscriptions.clear();subscriptionResources.clear();sampleKeys.clear();if(connection!=null)connection.active(false);String message="Dashboard stopped — "+error;retire();if(getIntent().getBooleanExtra("failure_signals",false))failureSignals.put(message);
  try{Files.write(new File(getFilesDir(),"report.json").toPath(),new JSONObject().put("registration",registry==null?JSONObject.NULL:registry.publicStatus()).put("editRevision",editRevision).put("failure",message).put("signals",failureSignals).put("subscriptions",0).toString().getBytes(StandardCharsets.UTF_8));}catch(Exception ignored){}
  main.post(()->{status.setText(message);status.setTextColor(0xff991b1b);status.setVisibility(View.VISIBLE);restart.setVisibility(View.VISIBLE);body.setVisibility(View.GONE);});
 }
 interface Completion{void done(JSONObject value,String error)throws Exception;}
 void rpc(String op,JSONObject args,Completion done)throws JSONException{
  if(connection!=null){long stamp=epoch;if(op.equals("action")){JSONObject wire=args.optJSONObject("wire");connection.write(args.optString("id","value"),wire,args.optString("actionId"),(v,e)->{if(stamp==epoch&&visible&&!failed)try{done.done(v,e);}catch(Exception x){fail(x);}});}else if(op.equals("run")){connection.run(args.getString("id"),args.getString("actionId"),(v,e)->{if(stamp==epoch&&visible&&!failed)try{done.done(v,e);}catch(Exception x){fail(x);}});}else{connection.request(op,args,(v,e)->{if(stamp==epoch&&visible&&!failed)try{done.done(v,e);}catch(Exception x){fail(x);}});}return;}
  args.remove("id");
  long stamp=epoch;JSONObject payload=new JSONObject().put("session","p1-dashboard").put("channel","android").put("epoch",epoch).put("id",++next).put("op",op).put("args",args);
  io.execute(()->{if(stamp!=epoch||destroyed||!visible||failed)return;JSONObject value=null;String error=null;
   try{HttpURLConnection c=(HttpURLConnection)new URL("http://127.0.0.1:"+port+"/rpc").openConnection();try{c.setRequestMethod("POST");c.setDoOutput(true);c.setConnectTimeout(5000);c.setReadTimeout(5000);c.setRequestProperty("Content-Type","application/json");try(OutputStream out=c.getOutputStream()){out.write(payload.toString().getBytes(StandardCharsets.UTF_8));}try(InputStream in=c.getInputStream()){JSONObject reply=new JSONObject(new String(readLimited(in,32768),StandardCharsets.UTF_8));if(reply.has("error"))error=reply.getString("error");else value=reply.getJSONObject("value");}}finally{c.disconnect();}}
   catch(Exception e){error=e.toString();}JSONObject result=value;String problem=error;
   worker.post(()->{if(stamp!=epoch||destroyed||!visible||failed)return;try{done.done(result,problem);}catch(Exception e){fail(e);}});
  });
 }
 void deliver(JSONObject message)throws Exception{if(visible&&!failed&&connection!=null&&message.optJSONObject("value")!=null&&message.getJSONObject("value").optJSONObject("value")!=null){eval("(()=>{let m="+message+";m.valueWire=TaliaValue.encode({...m.value,value:TaliaValue.decode(m.value.value)});TaliaVM.receive(JSON.stringify(m));})();'ok';",false,false);return;}if(visible&&!failed)eval("TaliaVM.receive("+JSONObject.quote(message.toString())+");'ok';",false,false);}
 void grant(String kind,String resource){
  if(!durable&&(!resource.equals("value")||kind.equals("runs")))throw new IllegalStateException("durable engine required");
  JSONObject grants=loaded==null?null:loaded.optJSONObject("grants");
  if(grants==null){if((kind.equals("reads")||kind.equals("writes"))&&resource.equals("value"))return;}
  else{JSONArray ids=grants.optJSONArray(kind);if(ids!=null)for(int n=0;n<ids.length();n++)if(resource.equals(ids.optString(n)))return;}
  throw new IllegalStateException("resource grant");
 }
 void syncResources(){if(connection!=null){connection.resources(subscriptionResources.values());connection.active(visible&&!failed&&!subscriptions.isEmpty());}}
 void request(JSONObject r)throws Exception{
  if(!visible||failed)return;long id=r.getLong("id");if(id<=lastRequest)throw new IllegalStateException("bridge replay");lastRequest=id;String op=r.getString("op");Object value=r.get("value");
  switch(op){
   case "subscribe":grant("reads",value.toString());if(subscriptions.size()>=16)throw new IllegalStateException("subscription budget");String sid="s"+id;subscriptions.put(sid,-1L);subscriptionResources.put(sid,value.toString());sampleKeys.remove(value.toString());lastSampleKey="";syncResources();deliver(new JSONObject().put("id",id).put("value",sid));break;
   case "unsubscribe":if(subscriptions.remove(value.toString())==null)throw new IllegalStateException("subscription ownership");subscriptionResources.remove(value.toString());syncResources();deliver(new JSONObject().put("id",id).put("value",JSONObject.NULL));break;
   case "read":grant("reads",value.toString());rpc("read",new JSONObject().put("id",value.toString()).put("tag","android-read"),(v,e)->deliver(new JSONObject().put("id",id).put(e==null?"value":"error",e==null?v:e)));break;
   case "write":{
    String resource=value instanceof JSONObject?((JSONObject)value).optString("id","value"):"value";grant("writes",resource);
    JSONObject wire=value instanceof JSONObject?((JSONObject)value).optJSONObject("wire"):null;if(wire!=null&&!durable)value=wire.getJSONArray("value").get(1);
    if(!durable&&(!(value instanceof Number)||((Number)value).doubleValue()!=((Number)value).longValue()||Math.abs(((Number)value).doubleValue())>1000000))throw new IllegalStateException("write value");
    if(actions.size()>=128)throw new IllegalStateException("action tracking limit");String action=nextActionId;nextActionId="a-"+UUID.randomUUID();actions.put(action,new JSONObject().put("status","unknown"));rpc("action",new JSONObject().put("id",resource).put("actionId",action).put("value",value).put("wire",durable?wire:null),(v,e)->{if(e==null)actions.put(action,v);deliver(new JSONObject().put("id",id).put(e==null?"value":"error",e==null?v:e));});break;
   }
   case "run":{
    if(!durable)throw new IllegalStateException("durable engine required");grant("runs",value.toString());
    if(actions.size()>=128)throw new IllegalStateException("action tracking limit");String action=nextActionId;nextActionId="a-"+UUID.randomUUID();actions.put(action,new JSONObject().put("status","unknown"));
    rpc("run",new JSONObject().put("id",value.toString()).put("actionId",action),(v,e)->{if(e==null)actions.put(action,v);deliver(new JSONObject().put("id",id).put(e==null?"value":"error",e==null?v:e));});break;
   }
   default:throw new IllegalStateException("operation not granted");
  }
 }
 void poll()throws Exception{if(connection!=null){for(JSONObject sample:new ArrayList<>(connection.values.values()))durableSample(sample);return;}if(polling||subscriptions.isEmpty())return;polling=true;
  rpc("read",new JSONObject().put("tag","android-subscription"),(v,e)->{polling=false;if(e!=null){externalError=e;return;}for(String id:new ArrayList<>(subscriptions.keySet()))if(v.getLong("revision")>subscriptions.get(id)){subscriptions.put(id,v.getLong("revision"));deliver(new JSONObject().put("event",id).put("value",v));}});
 }

 void durableSample(JSONObject value)throws Exception{
  if(connection==null||!connection.ready||!visible||failed)return;String resource=value.getString("id");if(!subscriptionResources.containsValue(resource))return;
  String key=connection.incarnation+":"+value.toString();if(key.equals(sampleKeys.get(resource)))return;sampleKeys.put(resource,key);
  JSONObject wire=new JSONObject(eval("TaliaValue.stringify({..."+value+",hasValue:"+value.optBoolean("hasValue",value.optBoolean("has_value"))+",value:TaliaValue.decode("+value.getJSONObject("value")+")})",true,false));
  for(String id:new ArrayList<>(subscriptions.keySet()))if(resource.equals(subscriptionResources.get(id)))deliver(new JSONObject().put("event",id).put("valueWire",wire));
 }
 JSONObject registryReport(String lifecycle)throws JSONException{return new JSONObject().put("dashboardId",loaded.optString("id",dashboardId)).put("packageRevision",loaded.optString("revision","loading")).put("lifecycle",lifecycle).put("foreground",visible).put("dirty",registryDirty||editRevision>0).put("editRevision",editRevision).put("updateAvailable",updateAvailable).put("assignmentRevision",assignmentRevision==0?JSONObject.NULL:assignmentRevision).put("cached",cachedBaseline);}
 void reportRegistry(boolean force){if(registry==null||loaded==null)return;try{registry.update(registryReport(failed?"failed":!visible||!started?"paused":"active"));registry.tick(force);}catch(Exception ignored){}}
 @Override public void onStart(){super.onStart();visible=true;if(worker!=null)worker.post(()->{reportRegistry(true);if(failed)return;try{if(pendingPackage!=null){installPackage(pendingPackage);return;}if(!started){start();return;}epoch++;polling=false;eval("TaliaVM.resume();'ok';",false,false);if(connection!=null){lastSampleKey="";sampleKeys.clear();syncResources();}subscriptions.replaceAll((k,v)->-1L);reconcileOutcomes();}catch(Exception e){fail(e);}});}
 void finishResume()throws Exception{eval("TaliaVM.reconcile("+new JSONArray(actions.values())+");'ok';",false,false);refresh();poll();}
 void reconcileOutcomes()throws Exception{if(actions.isEmpty()){finishResume();return;}int[] pending={actions.size()};for(String id:new ArrayList<>(actions.keySet()))rpc("status",new JSONObject().put("actionId",id),(v,e)->{if(e==null)actions.put(id,v);if(--pending[0]==0)finishResume();});}
 @Override public void onStop(){visible=false;if(worker!=null)worker.post(()->{reportRegistry(true);epoch++;polling=false;if(connection!=null)connection.active(false);if(started&&!failed)try{eval("TaliaVM.pause();'ok';",false,false);File reportFile=new File(getFilesDir(),"report.json");if(reportFile.exists()){JSONObject r=new JSONObject(new String(Files.readAllBytes(reportFile.toPath()),StandardCharsets.UTF_8));r.put("paused",true);r.put("subscriptions",0);Files.write(reportFile.toPath(),r.toString().getBytes(StandardCharsets.UTF_8));}}catch(Exception e){fail(e);}});super.onStop();}
 @Override protected void onNewIntent(android.content.Intent intent){super.onNewIntent(intent);
  if(BuildConfig.DEBUG&&intent.getBooleanExtra("renderer_checks",false))worker.post(()->{try{
   String source=asset("renderer.ui");String scenes=eval("JSON.stringify([TaliaUI.resolve(TaliaUI.compile("+JSONObject.quote(source)+"),{items:[{id:'a',label:'A',value:true},{id:'b',label:'B',value:false}]}),TaliaUI.resolve(TaliaUI.compile("+JSONObject.quote(source)+"),{items:[{id:'b',label:'B',value:false},{id:'a',label:'A',value:true}]})])",true,false);
   main.post(()->{JSONObject result;try{result=RendererChecks.run(this,root,new JSONArray(scenes));}catch(Exception e){result=new JSONObject();try{result.put("passed",false).put("error",e.toString());}catch(Exception ignored){}}try{Files.write(new File(getFilesDir(),"renderer-checks.json").toPath(),result.toString().getBytes(StandardCharsets.UTF_8));}catch(Exception ignored){}});
  }catch(Exception e){fail(e);}});
  if(BuildConfig.DEBUG&&intent.hasExtra("live"))worker.post(()->{if(!started||!visible||failed)return;try{editRevision++;registryDirty=true;reportRegistry(true);eval("TaliaVM.markDirty();\n"+intent.getStringExtra("live")+"\n;'ok';",false,false);refresh();}catch(Exception e){fail(e);}});
 }
 @Override public void onDestroy(){destroyed=true;if(registry!=null)registry.close();if(connection!=null)connection.close();io.shutdownNow();worker.removeCallbacksAndMessages(null);worker.post(MainActivity::retire);thread.quitSafely();main.removeCallbacksAndMessages(null);super.onDestroy();}
}
