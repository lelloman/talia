package com.lelloman.talia.dashboard;
import android.content.Context;
import org.json.*;
import java.io.*;
import java.net.*;
import java.nio.charset.StandardCharsets;
import java.util.UUID;
/** Host-only authenticated alert transport. Call on an I/O executor. */
final class AlertApi {
 static JSONObject request(Context context,int port,String op,JSONObject args)throws Exception{
  String token=context.getSharedPreferences("talia-alerts",0).getString("credential","");
  if(token.isEmpty())throw new IOException("Alert access is not configured");
  if(!op.equals("snapshot")&&!op.equals("history")&&!op.equals("config")&&!op.equals("audit")&&!args.has("requestId"))args.put("requestId",UUID.randomUUID().toString());
  HttpURLConnection c=(HttpURLConnection)new URL("http://127.0.0.1:"+port+"/alerts").openConnection();
  try{c.setRequestMethod("POST");c.setDoOutput(true);c.setConnectTimeout(5000);c.setReadTimeout(5000);c.setRequestProperty("Content-Type","application/json");c.setRequestProperty("Authorization","Bearer "+token);
   try(OutputStream out=c.getOutputStream()){out.write(new JSONObject().put("op",op).put("args",args).toString().getBytes(StandardCharsets.UTF_8));}
   try(InputStream in=c.getInputStream()){JSONObject r=new JSONObject(new String(MainActivity.readLimited(in,1048576),StandardCharsets.UTF_8));if(r.has("error"))throw new IOException(r.getString("error"));return r;}
  }finally{c.disconnect();}
 }
}
