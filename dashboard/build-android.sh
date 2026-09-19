#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
node compile.mjs examples/monitor.package.json generated/monitor.json
SDK="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-$HOME/Android/Sdk}}"
NDK="${TALIA_NDK:-$SDK/ndk/27.0.12077973}"
TOOLCHAIN="$NDK/toolchains/llvm/prebuilt/linux-x86_64"
ABI="${TALIA_ANDROID_ABI:-x86_64}"
case "$ABI" in
 x86_64) TARGET=x86_64-linux-android;;
 arm64-v8a) TARGET=aarch64-linux-android;;
 *) exit 1;;
esac
TARGET_ENV="${TARGET//-/_}"
LINKER_ENV="CARGO_TARGET_${TARGET_ENV^^}_LINKER"
env "CC_${TARGET_ENV}=$TOOLCHAIN/bin/${TARGET}26-clang" "AR_${TARGET_ENV}=$TOOLCHAIN/bin/llvm-ar" "$LINKER_ENV=$TOOLCHAIN/bin/${TARGET}26-clang" "BINDGEN_EXTRA_CLANG_ARGS=--sysroot=$TOOLCHAIN/sysroot --target=${TARGET}26" cargo build --manifest-path native/Cargo.toml --target "$TARGET" --locked --offline
mkdir -p "jniLibs/$ABI"
cp "native/target/$TARGET/debug/libtalia_dashboard_runtime.so" "jniLibs/$ABI/"
printf 'sdk.dir=%s\n' "$SDK" > android/local.properties
if [[ -z "${TALIA_GRADLE:-}" ]]; then
 for candidate in "$HOME"/.gradle/wrapper/dists/gradle-8.13-bin/*/gradle-8.13/bin/gradle; do TALIA_GRADLE="$candidate"; done
fi
"$TALIA_GRADLE" -p android --offline --no-daemon "-PtaliaAbi=$ABI" assembleDebug
