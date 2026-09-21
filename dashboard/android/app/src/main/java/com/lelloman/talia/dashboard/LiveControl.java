package com.lelloman.talia.dashboard;
import org.json.*;
import java.util.*;
import java.util.concurrent.*;
import android.os.SystemClock;

/** Host-only command routing; invocation JS has neither credentials nor UI/runtime handles. */
final class LiveControl {
 final MainActivity host;boolean busy;
 LiveControl(MainActivity host){this.host=host;}
 <T>T worker(Callable<T> action)throws Exception {FutureTask<T> task=new FutureTask<>(action);host.worker.post(task);try{return task.get(5,TimeUnit.SECONDS);}catch(Exception e){task.cancel(false);throw e;}}
 JSONObject send(JSONObject address,String op,String id,JSONObject extra)throws Exception {
  JSONObject body=new JSONObject(address.toString()).put("op",op);if(id!=null)body.put("commandId",id);
  if(extra!=null)for(Iterator<String> keys=extra.keys();keys.hasNext();){String key=keys.next();body.put(key,extra.get(key));}
  return host.registry.send(body).getJSONObject("value");
 }
 void tick(){if(busy||host.registry==null||!host.registry.connected)return;busy=true;
  try{final JSONObject address=host.registry.address("livePoll");host.io.execute(()->{try{JSONArray commands=send(address,"livePoll",null,null).getJSONArray("commands");for(int i=0;i<commands.length();i++)execute(address,commands.getJSONObject(i));}catch(Exception ignored){}finally{host.worker.post(()->busy=false);}});}catch(Exception e){busy=false;}
 }
 void local(String live,long until)throws Exception{if(host.destroyed||!host.visible||!live.equals(host.registry.live)||SystemClock.elapsedRealtime()>=until)throw new Exception("cancelled");}
 void check(JSONObject address,String id,String live,long until)throws Exception {worker(()->{local(live,until);return null;});send(address,"liveCheck",id,null);worker(()->{local(live,until);return null;});}
 JSONObject metadata()throws Exception{return worker(()->new JSONObject().put("liveInstanceId",host.registry.live).put("packageRevision",host.loaded.getString("revision")).put("editRevision",host.editRevision));}
 JSONObject report()throws Exception{return worker(()->new JSONObject().put("liveInstanceId",host.registry.live).put("foreground",host.visible).put("lifecycle",host.failed?"failed":host.started?"active":"paused").put("dirty",host.registryDirty||host.editRevision>0).put("editRevision",host.editRevision));}
 JSONObject inspect()throws Exception{return worker(()->{
  if(host.failed)return new JSONObject().put("state",host.lastVmSnapshot==null?new JSONObject("{\"version\":1,\"value\":[\"undefined\"]}"):host.lastVmSnapshot.getJSONObject("stateWire")).put("revision",host.lastVmSnapshot==null?0:host.lastVmSnapshot.optLong("revision")).put("failure","dashboard_failed");
  return new JSONObject(host.eval("(()=>{const s=TaliaVM.snapshot();return JSON.stringify({...s,state:TaliaValue.encode(s.state)})})()",false,false));
 });}
 JSONObject eval(String source,boolean reset)throws Exception{return worker(()->{JSONObject result=new JSONObject(MainActivity.evaluateLive(source,reset));if(result.has("error"))throw new Exception("validation_failed");return result;});}
 void execute(JSONObject address,JSONObject command){String id=command.optString("id"),live=address.optString("live"),operation=command.optString("operation");long until=SystemClock.elapsedRealtime()+Math.min(5000,command.optLong("remainingMs"));
  try{
   JSONObject args=command.getJSONObject("arguments"),r=report();worker(()->{local(live,until);return null;});
   if(args.has("expectedEditRevision")&&args.getLong("expectedEditRevision")!=r.getLong("editRevision"))throw new Exception("conflict");
   JSONObject grants=worker(()->host.loaded.optJSONObject("grants"));if(grants==null)grants=new JSONObject("{\"reads\":[\"value\"],\"writes\":[\"value\"],\"runs\":[]}");
   JSONObject begun=send(address,"liveBegin",id,new JSONObject().put("report",r).put("grants",grants));check(address,id,live,until);
   if(operation.equals("live_inspect")){send(address,"liveFinish",id,new JSONObject().put("value",metadata().put("snapshot",inspect())));return;}
   if(operation.equals("live_reload")){
    JSONObject delivery=begun.getJSONObject("delivery"),pkg=delivery.getJSONObject("package");
    worker(()->{host.eval(host.asset("value.js")+"\n"+host.asset("ui.js")+"\nTaliaUI.validatePackage("+pkg+");'ok';",true,true);return null;});
    check(address,id,live,until);
    worker(()->{local(live,until);if(host.editRevision!=args.getLong("expectedEditRevision"))throw new Exception("conflict");if((host.registryDirty||host.editRevision>0)&&!args.optBoolean("discardDirty"))throw new Exception("dirty_ack_required");host.loadRequest++;host.installSaved(delivery);host.reportRegistry(true);return null;});
    while(!worker(()->host.registry.connected)&&SystemClock.elapsedRealtime()<until){worker(()->{host.reportRegistry(true);return null;});Thread.sleep(20);}
    send(address,"liveFinish",id,new JSONObject().put("value",metadata()));return;
   }
   worker(()->{local(live,until);host.editRevision++;host.registryDirty=true;host.eval("TaliaVM.markDirty();'ok';",false,false);host.reportRegistry(true);return null;});
   JSONObject snapshot=inspect();String initial="{revision:"+snapshot.getLong("revision")+",value:TaliaValue.decode("+snapshot.getJSONObject("state")+")}";
   String library=worker(()->host.asset("value.js")+"\nconst liveInitial="+initial+";\n"+host.asset("live.js"));
   check(address,id,live,until);
   JSONObject evaluated=eval(library+"\nTaliaLive.start("+JSONObject.quote(args.getString("source"))+");'ok';",true);
   for(;;){
    JSONArray out=evaluated.getJSONArray("out");for(int n=0;n<out.length();n++){
     JSONObject request=out.getJSONObject(n),reply=new JSONObject().put("id",request.getLong("id"));
     check(address,id,live,until);
     try{
      String op=request.getString("op");Object value;
      if(op.equals("commit")){JSONObject v=request.getJSONObject("value");value=worker(()->{local(live,until);String next=host.eval("String(TaliaVM.liveCommit("+v.getLong("revision")+",TaliaValue.decode("+v.getJSONObject("value")+")))",false,false);host.refresh();return new JSONObject("{\"version\":1,\"value\":[\"number\","+next+"]}");});}
      else{
       if(!Arrays.asList("read","write","run").contains(op))throw new Exception("forbidden");
       String resource=op.equals("write")?request.getJSONObject("value").getString("id"):request.getString("value");
       JSONObject effect=new JSONObject().put("op",op).put("id",resource).put("sequence",request.getLong("id"));if(op.equals("write"))effect.put("value",request.getJSONObject("value").getJSONObject("wire"));
       JSONObject result=send(address,"liveEffect",id,new JSONObject().put("effect",effect));
       String encode=op.equals("run")?"TaliaValue.stringify("+result+")":"TaliaValue.stringify({..."+result+",value:TaliaValue.decode("+result.getJSONObject("value")+")})";
       value=worker(()->new JSONObject(host.eval(encode,true,false)));
      }reply.put("value",value);
     }catch(Exception error){reply.put("error","operation_failed");}
     check(address,id,live,until);JSONObject response=eval("TaliaLive.receive("+JSONObject.quote(reply.toString())+");'ok';",false);
     JSONArray extra=response.getJSONArray("out");for(int j=0;j<extra.length();j++)out.put(extra.get(j));
    }
    check(address,id,live,until);evaluated=eval("TaliaLive.snapshot()",false);String result=evaluated.getString("value");
    if(!result.equals("null")){JSONObject done=new JSONObject(result);if(done.has("error"))worker(()->{host.fail(new Exception("Live execution failed"));return null;});send(address,"liveFinish",id,new JSONObject().put("value",metadata().put("error",done.opt("error"))));break;}
    Thread.sleep(20);
   }
  }catch(Exception e){try{String message=String.valueOf(e);String code=message.contains("conflict")?"conflict":message.contains("dirty_ack_required")?"dirty_ack_required":message.contains("cancelled")?"cancelled":"validation_failed";if(code.equals("validation_failed")&&(operation.equals("live_execute")||operation.equals("live_reload")&&!live.equals(host.registry.live)))worker(()->{host.fail(new Exception("Live execution failed"));return null;});send(address,"liveFinish",id,new JSONObject().put("value",metadata().put("error",code)));}catch(Exception ignored){}}
  finally{host.worker.post(MainActivity::retireLive);}
 }
}
