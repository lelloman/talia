#!/usr/bin/env python3
"""Copy only runtime assets into a new deployment root after build-web.sh."""
import pathlib, shutil, sys
root=pathlib.Path(__file__).resolve().parents[1]
out=pathlib.Path(sys.argv[1]); out.mkdir(parents=True,exist_ok=False)
files=list((root/'dashboard/web').glob('*.js'))+list((root/'dashboard/web').glob('*.css'))+[root/'dashboard/web/index.html',root/'dashboard/web/dist/worker.js',root/'dashboard/web/dist/ng.wasm',root/'dashboard/web/dist/chrome.js',root/'dashboard/web/dist/chrome.css']+list((root/'dashboard/shared').glob('*.js'))+list((root/'engine/shared').glob('*.js'))+list((root/'assets/brand').glob('*.svg'))
for file in files:
 target=out/file.relative_to(root);target.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(file,target)
