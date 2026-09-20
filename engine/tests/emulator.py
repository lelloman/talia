#!/usr/bin/env python3
"""Own a disposable emulator/AVD, leaving the user's AVDs untouched."""
import pathlib,tempfile,subprocess,os,signal,shutil
sdk=pathlib.Path(os.environ.get('ANDROID_HOME',str(pathlib.Path.home()/'Android/Sdk')))
root=pathlib.Path(tempfile.mkdtemp(prefix='talia-p2-avd-'));avd=root/'TaliaP1.avd';avd.mkdir()
(avd/'config.ini').write_text('''AvdId=TaliaP1
abi.type=x86_64
hw.cpu.arch=x86_64
hw.cpu.ncore=4
hw.ramSize=2048
hw.lcd.width=1080
hw.lcd.height=2400
hw.lcd.density=420
hw.keyboard=yes
hw.gpu.enabled=yes
hw.gpu.mode=swiftshader_indirect
hw.sdCard=no
disk.dataPartition.size=3G
image.sysdir.1=system-images/android-36.1/google_apis_playstore/x86_64/
tag.id=google_apis_playstore
target=android-36.1
''')
(root/'TaliaP1.ini').write_text(f'path={avd}\ntarget=android-36.1\n')
def stop(*args):raise KeyboardInterrupt
signal.signal(signal.SIGTERM,stop)
p=None
try:
 p=subprocess.Popen([str(sdk/'emulator/emulator'),'-avd','TaliaP1','-port','5570','-no-window','-no-snapshot','-no-audio','-no-boot-anim','-gpu','swiftshader','-feature','-Vulkan'],env={**os.environ,'ANDROID_AVD_HOME':str(root),'ANDROID_HOME':str(sdk)})
 result=p.wait()
 if result:raise SystemExit(result)
except KeyboardInterrupt:pass
finally:
 if p and p.poll() is None:p.terminate();p.wait(timeout=30)
 shutil.rmtree(root)
