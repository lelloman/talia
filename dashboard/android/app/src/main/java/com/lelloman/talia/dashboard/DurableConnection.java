package com.lelloman.talia.dashboard;
import android.os.Handler;
import org.json.*;
import java.net.*;
import java.io.*;
import java.nio.charset.StandardCharsets;
import java.util.*;
import java.util.concurrent.ExecutorService;
import java.util.function.Consumer;
/** Activity-owned connection; subscription reconciliation is serialized independently of guest I/O. */
final class DurableConnection {
 static final class ApiFailure extends Exception{ApiFailure(String message){super(message);}}
 interface Completion {void done(JSONObject value,String error);}
 final Handler worker;final ExecutorService io;final int port;final Consumer<String> status;final Consumer<JSONObject> sample;
 final Map<String,JSONObject> actions,values=new LinkedHashMap<>();final String client="android-"+UUID.randomUUID();
 final Set<String> resources=new LinkedHashSet<>(Collections.singletonList("value")),subscribed=new LinkedHashSet<>();
 volatile long epoch=1;volatile String incarnation="";volatile boolean closed;boolean ready,busy,ever,active;long nextTry,onlineUntil,revision=-1;String state="connecting...",monitoringError;JSONObject current;
 DurableConnection(Handler worker,ExecutorService io,int port,Map<String,JSONObject> actions,Consumer<String> status,Consumer<JSONObject> sample){this.worker=worker;this.io=io;this.port=port;this.actions=actions;this.status=status;this.sample=sample;}
 void status(String text){state=text;status.accept(text);}
 JSONObject send(String op,JSONObject args,long generation,String identity)throws Exception{
  if(closed||generation!=epoch)throw new IOException("obsolete request");
  JSONObject body=new JSONObject().put("version",1).put("client",client).put("epoch",generation).put("incarnation",identity).put("op",op).put("args",args);
  HttpURLConnection c=(HttpURLConnection)new URL("http://127.0.0.1:"+port+"/engine").openConnection();try{c.setRequestMethod("POST");c.setDoOutput(true);c.setConnectTimeout(3000);c.setReadTimeout(6000);c.setRequestProperty("Content-Type","application/json");try(OutputStream out=c.getOutputStream()){out.write(body.toString().getBytes(StandardCharsets.UTF_8));}JSONObject reply;try(InputStream in=c.getInputStream()){reply=new JSONObject(new String(MainActivity.readLimited(in,2097152),StandardCharsets.UTF_8));}
   if(closed||generation!=epoch||reply.optLong("epoch")!=generation||reply.optInt("version")!=1)throw new IOException("obsolete response");
   if(!op.equals("hello")&&!identity.equals(reply.optString("incarnation")))throw new IOException("server incarnation changed");if(reply.has("error"))throw new ApiFailure(reply.getString("error"));return reply;
  }finally{c.disconnect();}
 }
 void adopt(JSONObject snapshot)throws JSONException{
  long rev=snapshot.getLong("revision");if(rev<revision)return;revision=rev;values.clear();JSONArray all=snapshot.getJSONArray("values");
  for(int i=0;i<all.length();i++){JSONObject v=all.getJSONObject(i);values.put(v.getString("id"),v);}current=values.get("value");monitoringError=snapshot.isNull("monitoringError")?null:snapshot.optString("monitoringError",null);
  for(JSONObject v:new ArrayList<>(values.values()))sample.accept(v);
 }
 void failed(){ready=false;nextTry=System.currentTimeMillis()+1000;status("disconnected");}
 void tick(){if(closed||busy||System.currentTimeMillis()<nextTry)return;busy=true;boolean reconnect=!ready;long generation=reconnect?++epoch:epoch;String identity=incarnation;
  Set<String> wanted=active?new LinkedHashSet<>(resources):new LinkedHashSet<>(),previous=reconnect?new LinkedHashSet<>():new LinkedHashSet<>(subscribed);
  List<String> pending=new ArrayList<>(actions.keySet());if(reconnect)status("connecting...");
  io.execute(()->{JSONObject snapshot=null;String newIdentity=identity,error=null;Map<String,JSONObject> outcomes=new HashMap<>();try{
   if(reconnect){JSONObject hello=send("hello",new JSONObject(),generation,identity);newIdentity=hello.getString("incarnation");}
   for(String id:previous)if(!wanted.contains(id))send("unsubscribe",object("id",id),generation,newIdentity);
   for(String id:wanted)if(!previous.contains(id))send("subscribe",object("id",id),generation,newIdentity);
   if(reconnect)for(String id:pending)outcomes.put(id,send("status",object("actionId",id),generation,newIdentity).getJSONObject("value"));
   snapshot=send(wanted.isEmpty()?"snapshot":"poll",new JSONObject(),generation,newIdentity).getJSONObject("value");
  }catch(Exception e){error=e.toString();}final JSONObject value=snapshot;final String inc=newIdentity,problem=error;
   worker.post(()->{busy=false;if(closed||generation!=epoch)return;if(problem!=null){failed();return;}try{
    if(reconnect){current=null;revision=-1;values.clear();incarnation=inc;actions.putAll(outcomes);}subscribed.clear();subscribed.addAll(wanted);ready=true;adopt(value);
    if(reconnect){onlineUntil=ever?System.currentTimeMillis()+3000:0;ever=true;}status(System.currentTimeMillis()<onlineUntil?"back online":"");
   }catch(Exception e){failed();}});
  });
 }
 void resources(Collection<String> ids){resources.clear();resources.addAll(ids);}
 void active(boolean value){active=value;}
 static JSONObject object(String key,Object value){try{return new JSONObject().put(key,value);}catch(Exception e){throw new IllegalArgumentException(e);}}
 void request(String op,JSONObject args,Completion completion){if(!ready){completion.done(null,"disconnected");return;}long generation=epoch;String identity=incarnation;
  io.execute(()->{JSONObject value=null;String error=null;boolean networkFailure=false;try{Object result=send(op,args,generation,identity).get("value");value=result instanceof JSONObject?(JSONObject)result:new JSONObject();}catch(Exception e){error=e.toString();networkFailure=!(e instanceof ApiFailure);}final JSONObject result=value;final String problem=error;final boolean disconnect=networkFailure;worker.post(()->{if(closed)return;if(generation!=epoch){completion.done(null,"obsolete request");return;}if(problem!=null&&disconnect)failed();completion.done(result,problem);});});
 }
 void action(String op,JSONObject args,String id,Completion completion){try{if(actions.size()>=128&&!actions.containsKey(id))throw new IllegalStateException("action tracking limit");actions.put(id,object("status","unknown"));args.put("actionId",id);request(op,args,(v,e)->{if(e==null){actions.put(id,v);if("failed".equals(v.optString("status"))){completion.done(null,v.optJSONObject("outcome").optString("error"));return;}}completion.done(v,e);});}catch(Exception e){completion.done(null,e.toString());}}
 void write(String resource,JSONObject wire,String id,Completion completion){JSONObject current=values.get(resource);if(current==null){completion.done(null,"value unavailable");return;}try{action("write",new JSONObject().put("id",resource).put("expected",current.getLong("revision")).put("value",wire),id,completion);}catch(Exception e){completion.done(null,e.toString());}}
 void run(String resource,String id,Completion completion){action("run",object("id",resource),id,completion);}
 void close(){closed=true;epoch++;}
}
