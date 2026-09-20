#!/usr/bin/env python3
"""Isolated real-HTTP collection, Watch, recovery and action identity qualification."""
import copy,json,os,pathlib,subprocess,tempfile,threading,time,urllib.request,sys
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
BIN=sys.argv[1] if len(sys.argv)>1 else '/tmp/talia-p3-target/debug/talia-engine'
ROOT=pathlib.Path(__file__).resolve().parents[2]
def wire(x):
 if x is None:n=['null']
 elif isinstance(x,bool):n=['boolean',x]
 elif isinstance(x,(float,int)):n=['number',x]
 elif isinstance(x,str):n=['string',x]
 elif isinstance(x,list):n=['array',[wire(v)['value'] for v in x]]
 else:n=['object',[[k,wire(v)['value']] for k,v in sorted(x.items())]]
 return {'version':1,'value':n}
class Fixture:
 def __init__(self):
  self.disk=12;self.probes=0;self.calls=0;owner=self
  class Handler(BaseHTTPRequestHandler):
   def log_message(self,*a):pass
   def do_GET(self):
    from urllib.parse import urlparse,parse_qs
    if self.headers.get('Authorization')!='Bearer fixture-secret':self.send_error(403);return
    url=urlparse(self.path);owner.calls+=1
    if url.path=='/api/v1/query':
     q=parse_qs(url.query)['query'][0];value={'cpu':42,'memory':64,'disk':owner.disk,'disk-y':20}[q]
     body={'status':'success','data':{'resultType':'vector','result':[{'metric':{'host':'fixture'},'value':[time.time(),str(value)]}]}}
    elif url.path=='/breakdown':owner.probes+=1;body={'db':60,'logs':20,'other':20}
    else:self.send_error(404);return
    data=json.dumps(body).encode();self.send_response(200);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
  self.server=ThreadingHTTPServer(('127.0.0.1',0),Handler);self.url=f'http://127.0.0.1:{self.server.server_port}';self.thread=threading.Thread(target=self.server.serve_forever,daemon=True);self.thread.start()
 def close(self):self.server.shutdown();self.server.server_close();self.thread.join()
def config(url):
 collect="{async run(ctx){const names=['cpu','memory','disk','disk-y'];const values=await Promise.all(names.map(query=>ctx.source('prom',{kind:'query',query})));for(let n=0;n<names.length;n++)await ctx.publish(names[n],values[n].result[0].samples[0][1]);}}"
 probe="{async run(ctx){const result=await ctx.source('http',{kind:'http',path:'/breakdown'});await ctx.publish('breakdown',result);return result}}"
 return {'version':1,'sources':[{'id':'prom','kind':'prometheus','url':url,'credential_ref':'fixture'},{'id':'http','kind':'http','url':url,'credential_ref':'fixture'}],
 'definitions':[{'id':'collect','version':1,'kind':'pipeline','source':collect},{'id':'probe','version':1,'kind':'pipeline','source':probe},{'id':'disk-watch','version':1,'kind':'watch','source':(ROOT/'engine/examples/disk-watch.js').read_text(),'parameter_change':'reset'}],
 'instances':[{'id':'collection','definition':'collect','sources':{'prom':'prom'},'outputs':{n:n for n in ['cpu','memory','disk','disk-y']},'schedule':{'kind':'interval','every_ms':200},'stale_after_ms':1000},
 {'id':'investigate','definition':'probe','sources':{'http':'http'},'outputs':{'breakdown':'breakdown'}},
 {'id':'disk-x','definition':'disk-watch','inputs':{'disk':'disk'},'actions':{'investigate':'investigate'},'params':wire({'high':10,'low':6,'recovery':12})},
 {'id':'disk-y','definition':'disk-watch','inputs':{'disk':'disk-y'},'actions':{'investigate':'investigate'},'params':wire({'high':10,'low':6,'recovery':12})}]}
class EngineProcess:
 def __init__(self,db,port=0):self.db=str(db);self.port=port;self.proc=None
 def start(self):
  env=dict(os.environ,TALIA_SECRET_fixture='fixture-secret');self.proc=subprocess.Popen([BIN,self.db,str(self.port),'--seed'],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,env=env)
  line=self.proc.stdout.readline()
  if not line:raise RuntimeError(self.proc.stderr.read())
  info=json.loads(line);self.port=info['port'];self.inc=info['incarnation'];return self
 def stop(self,kill=False):
  if self.proc and self.proc.poll() is None:(self.proc.kill if kill else self.proc.terminate)();self.proc.wait(timeout=5)
 def rpc(self,op,args=None):
  data=json.dumps(dict(version=1,client='p3-tests',epoch=1,incarnation=self.inc,op=op,args=args or {})).encode()
  with urllib.request.urlopen(urllib.request.Request(f'http://127.0.0.1:{self.port}/engine',data=data,headers={'Content-Type':'application/json'}),timeout=8) as r:return json.load(r)
 def ok(self,op,args=None):
  r=self.rpc(op,args);assert 'error' not in r,r;return r['value']
 def seed(self,c):
  self.ok('define',{'expected':0,'definition':{'id':'measurements','version':1,'kind':'stored','source':'','value_schema':'any','state_schema':'any','dependencies':[]}})
  for name in ['cpu','memory','disk','disk-y','breakdown']:
   self.ok('create',{'instance':dict(id=name,definition='measurements',params=wire({}),state=wire({}),value=wire(None),has_value=False,timestamp=0,quality='unknown',revision=1,generation=1,history_count=20,history_age_ms=60000)})
  self.ok('configureMonitoring',{'expected':0,'config':c})
def eventually(fn,timeout=6):
 end=time.monotonic()+timeout
 while time.monotonic()<end:
  if fn():return
  time.sleep(.03)
 raise AssertionError('condition not reached')
def main():
 fixture=Fixture()
 with tempfile.TemporaryDirectory(prefix='talia-p3-http-') as temp:
  e=EngineProcess(pathlib.Path(temp)/'engine.db')
  try:
   e.start();c=config(fixture.url);e.seed(c)
   # No read/subscribe requests while automatic work starts.
   time.sleep(.65);assert fixture.calls>=8
   assert e.ok('read',{'id':'cpu'})['value']==wire(42)
   assert e.ok('subscribe',{'id':'monitor.disk-x'})['values']
   assert 'fixture-secret' not in json.dumps(e.ok('monitoringConfig'))
   bad=copy.deepcopy(c);bad['version']=2;bad['sources'][0]['url']='file:///etc/passwd'
   assert 'error' in e.rpc('configureMonitoring',{'expected':1,'config':bad});assert e.ok('monitoringConfig')['version']==1
   assert 'error' in e.rpc('remove',{'id':'disk'});assert e.ok('read',{'id':'disk'})
   fixture.disk=5;eventually(lambda:fixture.probes==1);eventually(lambda:e.ok('read',{'id':'breakdown'})['has_value'])
   before=e.ok('read',{'id':'monitor.disk-x'});assert 'true' in json.dumps(before['value'])
   assert e.ok('history',{'id':'cpu'})
   old=e.inc;e.stop(kill=True);time.sleep(.4);e.start();assert old!=e.inc;time.sleep(.7);assert fixture.probes==1
   assert e.ok('read',{'id':'monitor.disk-x'})['value']==before['value']
   a=e.ok('run',{'id':'investigate','actionId':'manual-probe'});eventually(lambda:e.ok('status',{'actionId':'manual-probe'})['status']=='complete');assert fixture.probes==2
   assert e.ok('run',{'id':'investigate','actionId':'manual-probe'})['runId']==a['runId'];assert fixture.probes==2
   assert 'error' in e.rpc('run',{'id':'collection','actionId':'manual-probe'})
   # An unfinished run is unknown after crash, and status never dispatches it again.
   c=e.ok('monitoringConfig');c['version']+=1;c['definitions'][1]['version']+=1;c['definitions'][1]['source']="{async run(ctx){await ctx.source('http',{kind:'http',path:'/breakdown'});await ctx.sleep(3000)}}";e.ok('configureMonitoring',{'expected':1,'config':c})
   a=e.ok('run',{'id':'investigate','actionId':'crash-probe'});eventually(lambda:fixture.probes==3);e.stop(kill=True);e.start()
   assert e.ok('status',{'actionId':'crash-probe'})['status']=='unknown';e.ok('run',{'id':'investigate','actionId':'crash-probe'});time.sleep(.3);assert fixture.probes==3
   cancel=e.ok('run',{'id':'investigate','actionId':'cancel-probe'});assert e.ok('cancelRun',{'id':cancel['runId']})['status']=='cancelled'
   assert 'error' in e.rpc('resumeWatch',{'id':'collection'})
   print(json.dumps({'passed':True,'checks':['clientless collection','Prometheus and probe fixtures','hot configuration rollback','reference integrity','state subscriptions','history','restart flags','manual deduplication','unknown no replay','credential redaction']}))
  finally:e.stop();fixture.close()
if __name__=='__main__':main()
