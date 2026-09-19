#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
SDK="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-$HOME/Android/Sdk}}"
NDK="${TALIA_NDK:-$SDK/ndk/27.0.12077973}"
TOOLCHAIN="$NDK/toolchains/llvm/prebuilt/linux-x86_64"
ABI="${TALIA_ANDROID_ABI:-x86_64}"
case "$ABI" in
  x86_64) TARGET=x86_64-linux-android; CLANG_TARGET=x86_64-linux-android ;;
  arm64-v8a) TARGET=aarch64-linux-android; CLANG_TARGET=aarch64-linux-android ;;
  *) echo "Unsupported TALIA_ANDROID_ABI: $ABI" >&2; exit 1 ;;
esac
TARGET_ENV="${TARGET//-/_}"
LINKER_ENV="CARGO_TARGET_${TARGET_ENV^^}_LINKER"
env "CC_${TARGET_ENV}=$TOOLCHAIN/bin/${CLANG_TARGET}26-clang" \
    "AR_${TARGET_ENV}=$TOOLCHAIN/bin/llvm-ar" \
    "$LINKER_ENV=$TOOLCHAIN/bin/${CLANG_TARGET}26-clang" \
    "BINDGEN_EXTRA_CLANG_ARGS=--sysroot=$TOOLCHAIN/sysroot --target=${CLANG_TARGET}26" \
    cargo build --manifest-path native/Cargo.toml --target "$TARGET" --locked
mkdir -p "jniLibs/$ABI"
cp "native/target/$TARGET/debug/libtalia_runtime_spike.so" "jniLibs/$ABI/"
printf 'sdk.dir=%s\n' "$SDK" > android/local.properties
"${TALIA_GRADLE:?Set TALIA_GRADLE to Gradle 8.13 executable}" -p android --no-daemon "-PtaliaAbi=$ABI" assembleDebug
