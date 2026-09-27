# Talìa Android

Native Compose application shell with LelloDesign's actual scaffold, drawer,
account and section components. Appearance lives in Settings and persists across
launches. Paravoid's shell-owned update controls are accessible from Settings.

This first build is an implementation scaffold. Reports, chat, server sign-in,
automation and the Rust runtime are not connected yet. The UI says so explicitly.
No sample health data is represented as a real service result.

## Build

Requirements: JDK 17, Android SDK 36 with build-tools 36.1.0, Android API 30+ device, sibling
`../../paravoid-android` and `../../lellodesign/packages/compose` checkouts.
Override them with `PARAVOID_SOURCE_DIRECTORY` and
`LELLODESIGN_COMPOSE_DIRECTORY` (absolute paths recommended). The app consumes
LelloDesign through Gradle composite substitution, not copied Compose components.

```sh
export ANDROID_HOME="$HOME/Android/Sdk"
./gradlew :app:assembleNormalDebug
../scripts/publish-android-to-lellostore.sh --build-only
../scripts/publish-android-to-lellostore.sh --dry-run --json
```

The normal package is `com.lelloman.talia.normal`; the production Paravoid package
is `com.lelloman.talia`, version 0.1.0 (1), initial payload version 1. They can be
installed side by side. Paravoid uses complete packaging, embedded bootstrap,
APK-grant Store delivery, explicit update controls and default crash recovery.
No automatic check/download schedule or payload minification is enabled.

## Signing and trust

Create the local app identity once:

```sh
../scripts/setup-android-signing.py --store-trust /path/to/store-public-trust.json
```

This imports only the Store's public head/grant keys, generates separate APK
and VPK RSA-3072 keys, and writes private configuration under
`~/.config/talia/paravoid-release/` and ignored `signing.properties`. It refuses
to overwrite existing configuration. Back up these files securely; loss of the
APK key prevents compatible APK updates. Do not commit keys or passwords.
If interrupted during initial generation, inspect the partial files before any
manual repair; the tool deliberately does not silently regenerate identity.

Before subsequent payload updates, export and review a complete shell baseline:

```sh
./gradlew :app:exportParavoidAndroidReleaseParavoidCompleteBaseline
```

Retain the accepted contract outside build outputs, pass
`-PparavoidBaselineDirectory=/path/to/baseline` and increase
`-PparavoidPayloadVersion=N`. New manifest capabilities, trust or shell runtime
changes require a new shell generation. Increase Android versionCode/versionName
before uploading a new APK. These values are never incremented by the publisher.

See `../docs/android-publishing.md` for prebuilt shell and payload uploads and
`../docs/android-app-flows.md` for the implementation plan.

## Initial validation (2026-09-27)

Normal debug and signed Paravoid release builds passed. The shared LelloStore
publisher accepted the 22,881,175-byte shell APK with `--dry-run --json` (status
`valid`); nothing was uploaded. Both packages launched on the API 36.1 emulator.
The LelloDesign drawer and Settings navigation worked, and dark appearance
survived a full restart of the signed app.

Paravoid's update controls open, but report `UPDATE_SERVICE_UNAVAILABLE` and no
installed release. This remains unresolved; update delivery is not validated.
Do not treat the packaging dry-run as an end-to-end update test. The build uses
the sibling Paravoid source at commit `6770678`.
