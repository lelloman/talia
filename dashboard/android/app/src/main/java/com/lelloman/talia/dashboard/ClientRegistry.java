package com.lelloman.talia.dashboard;

import android.content.SharedPreferences;
import android.os.Handler;
import org.json.*;
import java.net.*;
import java.io.*;
import java.nio.charset.StandardCharsets;
import java.security.SecureRandom;
import java.util.UUID;
import java.util.concurrent.ExecutorService;

/** Host-only registration. Credentials never enter the native JS context or diagnostic reports. */
final class ClientRegistry {
 final SharedPreferences prefs; final Handler worker; final ExecutorService io; final int port;
 final String credential,slot,owner; String clientId,name,live,previous; long epoch,sequence,last;
 boolean busy,connected;volatile boolean closed; JSONObject report; String error;
 ClientRegistry(SharedPreferences prefs,Handler worker,ExecutorService io,int port){
  this.prefs=prefs;this.worker=worker;this.io=io;this.port=port;
  credential=prefs.getString("credential",secret());slot=prefs.getString("slot",UUID.randomUUID().toString());owner=prefs.getString("owner",secret());
  clientId=prefs.getString("clientId",null);name=prefs.getString("name","Android dashboard");live=prefs.getString("live",null);epoch=prefs.getLong("epoch",0);
  prefs.edit().putString("credential",credential).putString("slot",slot).putString("owner",owner).commit();
 }
 static String secret(){byte[] bytes=new byte[32];new SecureRandom().nextBytes(bytes);StringBuilder b=new StringBuilder();for(byte v:bytes)b.append(String.format("%02x",v&255));return b.toString();}
 void replace(JSONObject next){previous=live;live=UUID.randomUUID().toString();report=next;connected=false;sequence=0;epoch++;last=0;prefs.edit().putString("live",live).putLong("epoch",epoch).commit();}
 void update(JSONObject next){report=next;}
 JSONObject address(String op)throws JSONException{return new JSONObject().put("op",op).put("slot",slot).put("owner",owner).put("live",live).put("epoch",epoch);}
 JSONObject send(JSONObject body)throws Exception {
  HttpURLConnection c=(HttpURLConnection)new URL("http://127.0.0.1:"+port+"/clients").openConnection();
  try {c.setRequestMethod("POST");c.setDoOutput(true);c.setConnectTimeout(5000);c.setReadTimeout(5000);c.setRequestProperty("Content-Type","application/json");c.setRequestProperty("Authorization","Bearer "+credential);
   try(OutputStream out=c.getOutputStream()){out.write(body.toString().getBytes(StandardCharsets.UTF_8));}
   try(InputStream in=c.getInputStream()){JSONObject reply=new JSONObject(new String(MainActivity.readLimited(in,300000),StandardCharsets.UTF_8));if(reply.has("error"))throw new ServerError(reply.getString("error"));return reply;}
  }finally{c.disconnect();}
 }
 static final class ServerError extends IOException {ServerError(String code){super(code);}}
 JSONObject slotRequest(String op)throws JSONException{return new JSONObject().put("op",op).put("slot",slot).put("owner",owner);}
 JSONObject assignment()throws Exception{JSONObject r=send(new JSONObject().put("op","register").put("name",name).put("platform","android")).getJSONObject("value");clientId=r.getString("clientId");name=r.getString("name");prefs.edit().putString("clientId",clientId).putString("name",name).commit();return send(slotRequest("openSlot")).getJSONObject("value");}
 JSONObject prepare()throws Exception{assignment();return send(slotRequest("delivery")).getJSONObject("value");}
 void confirm(long revision)throws Exception{send(slotRequest("confirmDelivery").put("revision",revision));}
 void select(String dashboard,JSONObject params)throws Exception{JSONObject a=assignment();send(slotRequest("select").put("expected",a.getLong("revision")).put("assignment",new JSONObject().put("dashboardId",dashboard).put("params",params).put("presentation",new JSONObject())));}
 String cacheName(){return "baseline-"+clientId+"-"+slot+".json";}
 void tick(boolean force){
  long now=android.os.SystemClock.elapsedRealtime();if(closed||report==null||busy||!force&&now-last<3000)return;busy=true;last=now;
  final String stamp=live;final boolean connect=!connected;final JSONObject payload,enroll;
  try{enroll=new JSONObject().put("op","register").put("name",name).put("platform","android");
   if(connect){epoch++;prefs.edit().putLong("epoch",epoch).commit();payload=address("connect").put("previous",previous==null?JSONObject.NULL:previous).put("report",report);}
   else payload=address("report").put("sequence",++sequence).put("report",report);
  }catch(Exception e){busy=false;error="invalid report";return;}
  io.execute(()->{JSONObject registration=null;String problem=null;
   try{if(connect){registration=send(enroll).getJSONObject("value");JSONArray slots=send(new JSONObject().put("op","status")).getJSONObject("value").getJSONArray("slots");for(int i=0;i<slots.length();i++){JSONObject s=slots.getJSONObject(i);if(slot.equals(s.getString("slotId")))payload.put("previous",s.getString("liveInstanceId"));}}send(payload);}catch(Exception e){problem=e.getMessage();}
   final JSONObject registered=registration;final String failure=problem;
   worker.post(()->{busy=false;if(closed||!stamp.equals(live))return;
    if(registered!=null){clientId=registered.optString("clientId");name=registered.optString("name");prefs.edit().putString("clientId",clientId).putString("name",name).commit();}
    connected=failure==null;error=failure;if(connected&&connect){sequence=0;previous=null;}
   });
  });
 }
 JSONObject publicStatus()throws JSONException{return new JSONObject().put("clientId",clientId==null?JSONObject.NULL:clientId).put("name",name).put("slotId",slot).put("liveInstanceId",live==null?JSONObject.NULL:live).put("connected",connected).put("error",error==null?JSONObject.NULL:error);}
 void close(){closed=true;connected=false;/* An abrupt process death is bounded by the server lease. */}
}
