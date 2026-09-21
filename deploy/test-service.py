#!/usr/bin/env python3
"""OIDC protocol, browser-session and permission regression checks."""
import pathlib,subprocess
root=pathlib.Path(__file__).resolve().parents[1]
subprocess.run(['cargo','test','--manifest-path','engine/Cargo.toml','--locked','--offline','--target-dir','/tmp/talia-p3-target','--bin','talia-engine','--','--test-threads=1'],cwd=root,check=True)
subprocess.run(['cargo','test','--manifest-path','engine/Cargo.toml','--locked','--offline','--target-dir','/tmp/talia-p3-target','--lib','browser_session_tests'],cwd=root,check=True)
