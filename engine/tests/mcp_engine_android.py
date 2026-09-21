#!/usr/bin/env python3
"""MCP-written Infinity must remain visible through native subscription/rendering."""
import json,re,subprocess,sys,time,xml.etree.ElementTree as ET
serial,port=sys.argv[1:3]
if not serial.startswith('emulator-'):raise SystemExit('emulator required')
package='com.lelloman.talia.dashboard'
def adb(*args,check=True):return subprocess.run(['adb','-s',serial,*args],capture_output=True,text=True,check=check).stdout
def report():
 try:return json.loads(adb('shell','run-as',package,'cat','files/report.json'))
 except (ValueError,subprocess.CalledProcessError):return {}
def wait(fn):
 end=time.monotonic()+40
 while time.monotonic()<end:
  if fn():return
  time.sleep(.2)
 raise AssertionError(report())
def xml():
 adb('shell','uiautomator','dump','/sdcard/talia-mcp-engine.xml');return ET.fromstring(adb('shell','cat','/sdcard/talia-mcp-engine.xml'))
try:
 adb('install','-r','dashboard/android/app/build/outputs/apk/debug/app-debug.apk');adb('reverse',f'tcp:{port}',f'tcp:{port}')
 adb('shell','am','start','-W','-n',package+'/.MainActivity','--ei','port',port,'--ez','durable','true','--es','dashboard','monitor')
 wait(lambda:report().get('registration',{}).get('connected') and 'Infinity' in json.dumps(report().get('stateWire')))
 wait(lambda:any('∞' in n.get('content-desc','') or '∞' in n.get('text','') for n in xml().iter('node')))
 assert not report()['dirty'];assert all('WebView' not in n.get('class','') for n in xml().iter('node'))
 print(json.dumps({'passed':True,'checks':['MCP write observed by native subscription','exceptional value visible in chart','native controls and clean authored VM']}))
finally:
 adb('shell','am','force-stop',package,check=False);adb('uninstall',package,check=False);adb('reverse','--remove',f'tcp:{port}',check=False);adb('shell','rm','-f','/sdcard/talia-mcp-engine.xml',check=False)
