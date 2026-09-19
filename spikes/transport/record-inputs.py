#!/usr/bin/env python3
"""Record source provenance only after collecting the matching test reports."""
import datetime,hashlib,json
from pathlib import Path
root=Path(__file__).resolve().parent
paths=['server/Cargo.toml','server/Cargo.lock','server/src/main.rs',
       'shared/client.js','shared/suite.js','web/worker.js','web/index.html',
       'build-web.sh','run-browser.mjs','run-android.py','test-server.py',
       '../runtime/native/Cargo.toml','../runtime/native/Cargo.lock',
       '../runtime/native/src/lib.rs','../runtime/native/src/main.rs','../runtime/native/src/transport.rs',
       '../runtime/package-lock.json','../runtime/build-android.sh','../runtime/android/app/build.gradle',
       '../runtime/android/app/src/main/AndroidManifest.xml',
       '../runtime/android/app/src/main/java/com/lelloman/talia/spike/TransportActivity.java']
result={'date':str(datetime.date.today()),'sha256':{p:hashlib.sha256((root/p).read_bytes()).hexdigest() for p in paths}}
(root/'results/inputs.json').write_text(json.dumps(result,indent=2)+'\n')
