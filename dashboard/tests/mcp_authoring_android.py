#!/usr/bin/env python3
"""Load and interact with the MCP-authored package on an explicitly selected emulator."""
import json,re,subprocess,sys,time,xml.etree.ElementTree as ET
serial,port=sys.argv[1:3]
if not serial.startswith('emulator-'):raise SystemExit('emulator required')
package='com.lelloman.talia.dashboard'
def adb(*args,check=True):return subprocess.run(['adb','-s',serial,*args],capture_output=True,text=True,check=check).stdout
def report():
 try:return json.loads(adb('shell','run-as',package,'cat','files/report.json'))
 except (ValueError,subprocess.CalledProcessError):return {}
def wait(fn):
 end=time.monotonic()+45
 while time.monotonic()<end:
  if fn():return
  time.sleep(.2)
 raise AssertionError(report())
try:
 adb('install','-r','dashboard/android/app/build/outputs/apk/debug/app-debug.apk');adb('reverse',f'tcp:{port}',f'tcp:{port}')
 adb('shell','am','start','-W','-n',package+'/.MainActivity','--ei','port',port,'--ez','durable','true','--es','dashboard','authored')
 wait(lambda:report().get('dashboardId')=='authored' and report().get('state',{}).get('n')==0)
 adb('shell','uiautomator','dump','/sdcard/talia-mcp.xml')
 root=ET.fromstring(adb('shell','cat','/sdcard/talia-mcp.xml'))
 for node in root.iter('node'):
  if node.get('text')=='Increment':
   x1,y1,x2,y2=map(int,re.findall(r'\d+',node.get('bounds')));adb('shell','input','tap',str((x1+x2)//2),str((y1+y2)//2));break
 else:raise AssertionError('missing Increment button')
 wait(lambda:report().get('state',{}).get('n')==2)
 r=report();assert not r.get('failure') and not r['dirty'];assert r['definitionRevision'].startswith('catalog-')
 print(json.dumps({'passed':True,'checks':['MCP-authored saved package on native emulator','shared UI button invokes shared VM/function','clean runtime after authored interaction']}))
finally:
 adb('shell','am','force-stop',package,check=False);adb('uninstall',package,check=False);adb('reverse','--remove',f'tcp:{port}',check=False);adb('shell','rm','-f','/sdcard/talia-mcp.xml',check=False)
