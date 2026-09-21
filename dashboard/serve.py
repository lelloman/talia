#!/usr/bin/env python3
"""Loopback development host. Reuses the P0 Rust server; never expose publicly."""
import http.server,json,pathlib,subprocess,urllib.request,urllib.parse,sys,signal,re,os
ROOT=pathlib.Path(__file__).resolve().parents[1]
durable=os.environ.get('TALIA_ENGINE_DB')
command=[os.environ.get('TALIA_ENGINE_BIN',str(ROOT/'engine/target/debug/talia-engine')),durable,'0','--seed'] if durable else [str(ROOT/'spikes/transport/server/target/debug/talia-transport-spike'),'0']
external=os.environ.get('TALIA_ENGINE_PORT')
server=None if external else subprocess.Popen(command,stdout=subprocess.PIPE,text=True)
port=int(external) if external else json.loads(server.stdout.readline())['port']
def stop(*args):raise KeyboardInterrupt
signal.signal(signal.SIGTERM,stop)
class Handler(http.server.SimpleHTTPRequestHandler):
 def __init__(self,*a,**kw):super().__init__(*a,directory=str(ROOT),**kw)
 def do_GET(self):
  parsed=urllib.parse.urlparse(self.path)
  if parsed.path=='/dashboard/package.json':
   if durable:self.send_error(404,'Use authenticated client delivery');return
   dashboard=urllib.parse.parse_qs(parsed.query).get('dashboard',['monitor'])[0]
   if not re.fullmatch(r'[A-Za-z][A-Za-z0-9_-]{0,63}',dashboard):self.send_error(400);return
   try:body=pathlib.Path(sys.argv[2] if len(sys.argv)>2 else ROOT/f'dashboard/generated/{dashboard}.json').read_bytes()
   except OSError:self.send_error(404);return
   self.send_response(200);self.send_header('Content-Type','application/json');self.send_header('Cache-Control','no-store');self.end_headers();self.wfile.write(body);return
  if self.path=='/' and not durable:self.send_response(302);self.send_header('Location','/dashboard/web/index.html?engine=legacy');self.end_headers();return
  if self.path=='/':self.path='/dashboard/web/index.html'
  super().do_GET()
 def do_POST(self):
  if self.path not in ['/rpc','/engine','/clients','/alerts']:self.send_error(404);return
  size=int(self.headers.get('Content-Length','0'))
  if size>262144:self.send_error(413);return
  try:
   request=urllib.request.Request(f'http://127.0.0.1:{port}{self.path}',data=self.rfile.read(size),headers={'Content-Type':'application/json', 'Authorization':self.headers.get('Authorization','')})
   with urllib.request.urlopen(request,timeout=6) as r:body=r.read()
   self.send_response(200);self.send_header('Content-Type','application/json');self.end_headers();self.wfile.write(body)
  except (BrokenPipeError,ConnectionResetError):pass
  except Exception:self.send_error(502)
 def log_message(self,*a):pass
try:
 with http.server.ThreadingHTTPServer(('127.0.0.1',int(sys.argv[1]) if len(sys.argv)>1 else 18744),Handler) as http:
  print(json.dumps({'port':http.server_port}),flush=True);http.serve_forever()
except KeyboardInterrupt:pass
finally:
 if server:server.terminate();server.wait(timeout=10)
