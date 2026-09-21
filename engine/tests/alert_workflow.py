#!/usr/bin/env python3
"""Real engine/MCP/Prometheus pipeline with local SMTP, Telegram and FCM fixtures."""
import copy,json,os,pathlib,socketserver,subprocess,tempfile,threading,time
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
import monitoring
from monitoring import Fixture,EngineProcess,config,eventually
from mcp_authoring import MCP,provision,BIN
monitoring.BIN=str(BIN/'talia-engine')
class Providers:
 def __init__(self):
  self.email=0;self.telegram=0;self.push=0;self.posts=[];owner=self
  class Http(BaseHTTPRequestHandler):
   def log_message(self,*a):pass
   def do_POST(self):
    data=self.rfile.read(int(self.headers.get('Content-Length',0)));status=200
    if self.path=='/token':assert b'assertion=' in data;result={'access_token':'fixture-oauth'}
    elif self.path=='/v1/projects/fixture/messages:send':
     assert self.headers['Authorization']=='Bearer fixture-oauth';message=json.loads(data)['message'];assert message['token']=='fixture-device-token';assert message['data']['key']=='disk-pressure';owner.push+=1;result={'name':'fixture/accepted'}
    elif self.path=='/botfixture-token/sendMessage':
     owner.telegram+=1
     if owner.telegram==1:status=429;result={'ok':False,'parameters':{'retry_after':1}}
     else:result={'ok':True,'result':{'message_id':owner.telegram}}
    else:status=404;result={}
    body=json.dumps(result).encode();self.send_response(status);self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
  class SMTP(socketserver.StreamRequestHandler):
   def handle(self):
    self.wfile.write(b'220 fixture ESMTP\r\n');data=False
    while True:
     line=self.rfile.readline()
     if not line:break
     if data:
      if line==b'.\r\n':owner.email+=1;self.wfile.write(b'250 accepted\r\n');data=False
     elif line.startswith(b'DATA'):data=True;self.wfile.write(b'354 continue\r\n')
     elif line.startswith(b'QUIT'):self.wfile.write(b'221 bye\r\n');break
     else:self.wfile.write(b'250 fixture\r\n')
  self.http=ThreadingHTTPServer(('127.0.0.1',0),Http);self.smtp=socketserver.ThreadingTCPServer(('127.0.0.1',0),SMTP);self.threads=[]
  for server in [self.http,self.smtp]:t=threading.Thread(target=server.serve_forever,daemon=True);t.start();self.threads.append(t)
  self.url=f'http://127.0.0.1:{self.http.server_port}'
 def close(self):
  for server in [self.http,self.smtp]:server.shutdown();server.server_close()
  for t in self.threads:t.join()
 def counts(self):return (self.email,self.telegram,self.push)
def main():
 metrics=Fixture();providers=Providers();old=os.environ.get('TALIA_ALERT_PROVIDERS');checks=[]
 with tempfile.TemporaryDirectory(prefix='talia-alert-workflow-') as tmp:
  tmp=pathlib.Path(tmp);e=EngineProcess(tmp/'engine.db');m=None
  try:
   key=tmp/'key.pem';subprocess.run(['openssl','genpkey','-algorithm','RSA','-pkeyopt','rsa_keygen_bits:2048','-out',str(key)],check=True,capture_output=True)
   account=tmp/'account.json';account.write_text(json.dumps({'client_email':'fixture@example.test','private_key':key.read_text(),'token_uri':providers.url+'/token'}))
   credentials=tmp/'providers.json';credentials.write_text(json.dumps({'mail':{'kind':'smtp','host':'127.0.0.1','port':providers.smtp.server_address[1],'tls':'none_loopback','from':'talia@example.test'},'chat':{'kind':'telegram','token':'fixture-token','base_url':providers.url},'push':{'kind':'fcm','project':'fixture','service_account':str(account),'base_url':providers.url}}));os.environ['TALIA_ALERT_PROVIDERS']=str(credentials)
   token=provision(e.db,tmp,'operator',[{'family':'alerts','actions':['read','observe','acknowledge','configure','register','silence','audit'],'scope':{'kind':'all'}}]);e.start();e.seed(config(metrics.url));m=MCP(e.port,token)
   assert {t['name'] for t in m.rpc('tools/list',{})['result']['tools']} >= {'alerts_policy_save','alerts_binding_save','alerts_snapshot','alerts_acknowledge','alerts_device_status'}
   def tool(op,args,ok=True):return m.tool('alerts_'+op,args,ok)
   def snapshot():return tool('snapshot',{})
   for id,channel,target in [('mail','email','operator@example.test'),('chat','telegram','123'),('push','push','user:operator')]:tool('destination_save',{'requestId':'destination-'+id,'expected':0,'destination':{'id':id,'version':1,'channel':channel,'provider':id,'target':target}})
   tool('device_register',{'requestId':'device','expected':0,'device':{'id':'emulator-fixture','version':1,'token':'fixture-device-token'}})
   action={'id':'notify','destinations':['mail','chat','push'],'repeat_ms':3000,'retry_ms':100,'expiry_ms':10000,'until_ack':True}
   p={'id':'disk-policy','version':1,'source':"{async evaluate(ctx){let v=await ctx.read('disk');if(!v.hasValue||v.quality!=='good'||ctx.now()-v.timestamp>2000)throw Error('missing');return {active:v.value<10,stage:v.value<6?'critical':'warning',severity:v.value<6?'critical':'warning',message:'Disk pressure'}}}",'stages':{'warning':{},'critical':{'reset_ack':True,'actions':[action]}},'recovery':[{'id':'recovered','destinations':['chat']}]}
   tool('policy_save',{'requestId':'policy','expected':0,'policy':p});tool('binding_save',{'requestId':'binding','expected':0,'binding':{'id':'disk','version':1,'policy':'disk-policy','key':'disk-pressure','inputs':{'disk':'disk'},'every_ms':100}})
   eventually(lambda:metrics.calls>=8);metrics.disk=8;eventually(lambda:len(snapshot()['alerts'])==1);assert providers.counts()==(0,0,0);checks.append('MCP-authored dashboard-only warning from automatic Prometheus collection')
   metrics.disk=5;eventually(lambda:providers.email>=1 and providers.telegram>=2 and providers.push>=1,timeout=10)
   a=snapshot()['alerts'][0];assert a['stage']=='critical';assert providers.email==1 and providers.push==1
   args={'requestId':'ack','key':a['key'],'occurrence':a['occurrence'],'expected':a['revision']};ack=tool('acknowledge',args);assert tool('acknowledge',args)==ack
   time.sleep(.3);baseline=providers.counts();time.sleep(3.1);assert providers.counts()==baseline;checks.append('three-channel escalation, independent Telegram retry and global acknowledgement')
   metrics.disk=12;eventually(lambda:not snapshot()['alerts'][0]['active']);eventually(lambda:providers.telegram>baseline[1]);checks.append('recovery notification and occurrence history')
   now=snapshot()['now'];tool('silence_save',{'requestId':'silence','expected':0,'silence':{'id':'maintenance','version':1,'key':'disk-pressure','until':now+1500,'reason':'fixture'}});baseline=providers.counts();metrics.disk=5;eventually(lambda:snapshot()['alerts'][0]['active']);time.sleep(.3);assert providers.counts()==baseline
   eventually(lambda:providers.email>baseline[0] and providers.push>baseline[2],timeout=5);a=snapshot()['alerts'][0];assert a['occurrence']==2 and a['acknowledgement'] is None
   tool('acknowledge',{'requestId':'ack-two','key':a['key'],'occurrence':2,'expected':a['revision']});checks.append('silence suppresses delivery while alerts remain visible; expiry resumes once')
   p2=copy.deepcopy(p);p2['version']=2;del p2['stages']['critical'];p2['source']=p2['source'].replace("stage:v.value<6?'critical':'warning'","stage:'warning'")
   failed=tool('policy_save',{'requestId':'bad-update','expected':1,'policy':p2},False);assert 'migration' in failed['error']
   tool('policy_save',{'requestId':'good-update','expected':1,'policy':p2,'migrations':{'critical':'warning'}});eventually(lambda:snapshot()['alerts'][0]['stage']=='warning');assert snapshot()['alerts'][0]['acknowledgement']
   m.close();m=None;e.stop(kill=True);e.start();m=MCP(e.port,token);time.sleep(.4);a=snapshot()['alerts'][0];assert a['stage']=='warning' and a['acknowledgement'] and a['occurrence']==2
   assert len(tool('history',{'key':'disk-pressure'})['occurrences'])==2;assert token.read_text() not in json.dumps(tool('audit',{}));checks.append('runtime migration, crash recovery, preserved acknowledgement and sanitized history/audit')
   print(json.dumps({'passed':True,'checks':checks,'providers':'local fixtures only'}))
  finally:
   if m:m.close()
   e.stop();metrics.close();providers.close()
   if old is None:os.environ.pop('TALIA_ALERT_PROVIDERS',None)
   else:os.environ['TALIA_ALERT_PROVIDERS']=old
if __name__=='__main__':main()
