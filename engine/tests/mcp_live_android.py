#!/usr/bin/env python3
import sys,subprocess,json,time,os
from mcp_authoring import MCP
from monitoring import eventually
engine,port,token=sys.argv[1:];serial=os.environ.get('TALIA_EMULATOR','emulator-5570');assert serial.startswith('emulator-')
package='com.lelloman.talia.dashboard'
def adb(*args,check=True):return subprocess.run(['adb','-s',serial,*map(str,args)],check=check,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True).stdout
def report():
 try:return json.loads(adb('shell','run-as',package,'cat','files/report.json'))
 except Exception:return {}
m=MCP(int(engine),token)
try:
 adb('install','-r','dashboard/android/app/build/outputs/apk/debug/app-debug.apk');adb('reverse',f'tcp:{port}',f'tcp:{port}')
 adb('shell','am','start','-W','-n',package+'/.MainActivity','--ei','port',port,'--ez','durable','true','--es','dashboard','monitor')
 eventually(lambda:report().get('registration',{}).get('connected'),timeout=40)
 r=report()['registration'];target=dict(clientId=r['clientId'],slotId=r['slotId'],liveInstanceId=r['liveInstanceId'])
 initial=m.tool('live_inspect',{'target':target});assert not initial['snapshot']['dirty']
 args=dict(target=target,expectedEditRevision=0,source='const s=ctx.state();await ctx.commit(s,{...s.value,title:{text:"Native live"},value:Infinity});await ctx.write("value",123);',requestId='android-edit')
 result=m.tool('live_execute',args);assert result['audit']['status']=='complete',result;assert m.tool('live_execute',args)==result
 eventually(lambda:report().get('dirty') and 'Infinity' in json.dumps(report().get('stateWire')))
 assert m.tool('engine_read',{'id':'value'})['sample']['value']['value'][1]==123
 slot=next(s for c in m.tool('clients_list',{})['clients'] for s in c['slots'] if s['liveInstanceId']==target['liveInstanceId'])
 reload=dict(target=target,expectedEditRevision=1,expectedAssignmentRevision=slot['desiredAssignment']['revision'],requestId='android-dirty')
 assert m.tool('live_reload',reload,False)['error']=='dirty_ack_required'
 result=m.tool('live_reload',{**reload,'discardDirty':True,'requestId':'android-reload'});assert result['audit']['status']=='complete',result
 eventually(lambda:report().get('registration',{}).get('liveInstanceId')!=target['liveInstanceId'] and not report().get('dirty',True))
 assert m.tool('engine_read',{'id':'value'})['sample']['value']['value'][1]==123
 assert m.tool('live_inspect',{'target':target},False)['error']=='stale_instance'
 r=report()['registration'];target['liveInstanceId']=r['liveInstanceId']
 adb('shell','input','keyevent','KEYCODE_HOME')
 eventually(lambda:any(s['liveInstanceId']==target['liveInstanceId'] and s['report']['lifecycle']=='paused' for c in m.tool('clients_list',{})['clients'] for s in c['slots']))
 assert m.tool('live_execute',dict(target=target,expectedEditRevision=0,source='await ctx.write("value",999)',requestId='android-paused'),False)['error']=='target_unavailable'
 print(json.dumps({'passed':True,'checks':['native clean inspection','isolated live state/engine mutation','retry deduplication','dirty acknowledgement','native dedicated reload/new identity','effects survive reload','paused rejection']}))
finally:
 m.close();adb('shell','am','force-stop',package,check=False);adb('uninstall',package,check=False);adb('reverse','--remove',f'tcp:{port}',check=False)
