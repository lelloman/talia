#!/usr/bin/env python3
"""Loopback development host. Reuses the P0 Rust server; never expose publicly."""
import http.server,json,pathlib,subprocess,urllib.request,sys,signal
ROOT=pathlib.Path(__file__).resolve().parents[1]
server=subprocess.Popen([str(ROOT/'spikes/transport/server/target/debug/talia-transport-spike'),'0'],stdout=subprocess.PIPE,text=True)
port=json.loads(server.stdout.readline())['port']
def stop(*args):raise KeyboardInterrupt
signal.signal(signal.SIGTERM,stop)
class Handler(http.server.SimpleHTTPRequestHandler):
 def __init__(self,*a,**kw):super().__init__(*a,directory=str(ROOT),**kw)
 def do_GET(self):
  if self.path=='/dashboard/package.json':
   try:body=pathlib.Path(sys.argv[2] if len(sys.argv)>2 else ROOT/'dashboard/generated/monitor.json').read_bytes()
   except OSError:self.send_error(404);return
   self.send_response(200);self.send_header('Content-Type','application/json');self.send_header('Cache-Control','no-store');self.end_headers();self.wfile.write(body);return
  if self.path=='/':self.path='/dashboard/web/index.html'
  super().do_GET()
 def do_POST(self):
  if self.path!='/rpc':self.send_error(404);return
  size=int(self.headers.get('Content-Length','0'))
  if size>32768:self.send_error(413);return
  try:
   request=urllib.request.Request(f'http://127.0.0.1:{port}/rpc',data=self.rfile.read(size),headers={'Content-Type':'application/json'})
   with urllib.request.urlopen(request,timeout=6) as r:body=r.read()
   self.send_response(200);self.send_header('Content-Type','application/json');self.end_headers();self.wfile.write(body)
  except Exception:self.send_error(502)
 def log_message(self,*a):pass
try:
 with http.server.ThreadingHTTPServer(('127.0.0.1',int(sys.argv[1]) if len(sys.argv)>1 else 18744),Handler) as http:
  print(json.dumps({'port':http.server_port}),flush=True);http.serve_forever()
except KeyboardInterrupt:pass
finally:server.terminate();server.wait(timeout=10)
