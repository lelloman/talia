# Android publishing

`scripts/publish-android-to-lellostore.sh` delegates validation, authentication
and upload to LelloStore's authoritative publisher. It creates drafts; it does
not publish, replace versions, increment versions or retry failed uploads.

## Current status

The production `android/` project now builds a native Compose scaffold in normal
and Paravoid modes. Configure its private signing material using
`scripts/setup-android-signing.py`; see `android/README.md`. Existing dashboard
and runtime-spike APKs are not selected automatically. This first build has no
server connection or Rust runtime integration yet.

## Prebuilt artifacts

```sh
# Validate an exact signed Paravoid shell locally (no login or upload).
./scripts/publish-android-to-lellostore.sh --artifact /path/to/shell.apk --dry-run --json

# Upload a shell draft; the publisher asks for confirmation.
./scripts/publish-android-to-lellostore.sh --artifact /path/to/shell.apk

# Upload a signed payload for an existing shell contract.
./scripts/publish-android-to-lellostore.sh --payload /path/to/payload.vpk \
  --package-name com.lelloman.talia --contract-id '<accepted-shell-contract-id>'
```

The package name above is the production identity, not the existing
spike's identity. Use the exact package and contract from the accepted shell.
The shared `upload-vpk` command has no dry-run mode; the wrapper rejects that
combination before invoking it. Payload packaging/signature/contract checks
belong to Paravoid's producer and the shared publisher, not a duplicate verifier.

## Release build contract

The default command builds `:app:assembleParavoidAndroidRelease` in `android/`
(or `TALIA_ANDROID_DIR`). It requires an executable `gradlew`, private
`signing.properties`, and these Paravoid inputs. The Gradle configuration
consumes them explicitly:

| Variable | Default |
| --- | --- |
| `PARAVOID_SIGNING_KEY` | `~/.config/talia/paravoid-release/talia-release.pk8` |
| `PARAVOID_SIGNING_KEY_ID` | `talia-release` |
| `PARAVOID_TRUST_POLICY` | `~/.config/talia/paravoid-release/trust.json` |
| `PARAVOID_UPDATE_BASE_URL` | `https://store.lelloman.com/api/paravoid/` |

Expected outputs under `app/build/outputs/paravoid/paravoidAndroidRelease/`:
`shell.apk` and embedded `payload.vpk`. `--build-only` stops before invoking the
publisher. Minification is not enabled by the wrapper. Payload-only builds and
baseline validation belong to the release Gradle configuration; supply their
result using `--payload`. APK and VPK signing keys are separate and private.

## Publisher configuration

Resolution order: `LELLOSTORE_PUBLISHER`, sibling
`../lellostore/scripts/publish-to-lellostore.py`, then
`$HOME/lelloprojects/lellostore/scripts/publish-to-lellostore.py`.
Defaults follow Accordomi: `https://store.lelloman.com`, issuer
`https://auth.lelloman.com`, and the shared public OIDC client ID. Override using
`LELLOSTORE_URL`, `LELLOSTORE_OIDC_ISSUER`, `LELLOSTORE_CLIENT_ID` or publisher CLI
options. The wrapper forwards arguments as separate values, including paths with
spaces. Explicitly authorized automation can pass `--yes --json`; the wrapper
never adds `--yes` itself. Conflicts and ambiguous failures stop without retry.

Based on Accordomi's Paravoid wrapper and the lellostore-publish skill. No shared
publisher code is copied into Talìa.
