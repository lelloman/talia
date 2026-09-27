#!/usr/bin/env python3
"""Create Talìa's local release identity once, using exported public Store keys."""
import argparse, base64, json, os, secrets, subprocess
from pathlib import Path
from cryptography.hazmat.primitives.asymmetric import rsa
from cryptography.hazmat.primitives import serialization

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--store-trust', type=Path, required=True,
                    help='Public trust JSON containing Store headKeys and grantKeys')
args = parser.parse_args()
root = Path(__file__).resolve().parents[1]
directory = Path.home() / '.config/talia/paravoid-release'
properties = root / 'android/signing.properties'
source = json.loads(args.store_trust.read_text())
for role in ('headKeys', 'grantKeys'):
    if not source.get(role):
        raise SystemExit(f'Missing {role} in public Store trust export')
# Never rotate an existing app identity silently.
if directory.exists() or properties.exists():
    raise SystemExit('Signing configuration already exists; refusing to overwrite it.')
os.umask(0o077)
directory.mkdir(parents=True)
key = rsa.generate_private_key(public_exponent=65537, key_size=3072)
(directory / 'talia-release.pk8').write_bytes(key.private_bytes(
    serialization.Encoding.DER, serialization.PrivateFormat.PKCS8, serialization.NoEncryption()))
public = key.public_key().public_bytes(serialization.Encoding.DER, serialization.PublicFormat.SubjectPublicKeyInfo)
trust = dict(version=1, applicationId='com.lelloman.talia', minimumPayloadVersion=1,
             minimumHeadRevision=1, releaseKeys={'talia-release': base64.b64encode(public).decode('ascii')},
             headKeys=source['headKeys'], grantKeys=source['grantKeys'])
(directory / 'trust.json').write_text(json.dumps(trust, indent=2) + '\n')
password = secrets.token_urlsafe(32)
env = {**os.environ, 'TALIA_KEYSTORE_PASSWORD': password}
subprocess.run(['keytool', '-genkeypair', '-keystore', str(directory / 'talia-release.p12'),
                '-storetype', 'PKCS12', '-alias', 'talia', '-keyalg', 'RSA', '-keysize', '3072',
                '-validity', '10000', '-dname', 'CN=Talia Android',
                '-storepass:env', 'TALIA_KEYSTORE_PASSWORD', '-keypass:env', 'TALIA_KEYSTORE_PASSWORD'],
               env=env, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
properties.write_text(f'storeFile={directory}/talia-release.p12\nstorePassword={password}\nkeyAlias=talia\nkeyPassword={password}\n')
print('Created private APK/VPK signing keys and public trust policy. Back up the signing directory and android/signing.properties securely.')
