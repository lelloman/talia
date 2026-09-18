#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
SDK="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-$HOME/Android/Sdk}}"
NDK="${TALIA_NDK:-$SDK/ndk/27.0.12077973}"
TOOLCHAIN="$NDK/toolchains/llvm/prebuilt/linux-x86_64"
export CC_x86_64_linux_android="$TOOLCHAIN/bin/x86_64-linux-android26-clang"
export AR_x86_64_linux_android="$TOOLCHAIN/bin/llvm-ar"
export CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER="$CC_x86_64_linux_android"
export BINDGEN_EXTRA_CLANG_ARGS="--sysroot=$TOOLCHAIN/sysroot --target=x86_64-linux-android26"
cargo build --manifest-path native/Cargo.toml --target x86_64-linux-android --locked
mkdir -p jniLibs/x86_64
cp native/target/x86_64-linux-android/debug/libtalia_runtime_spike.so jniLibs/x86_64/
printf 'sdk.dir=%s\n' "$SDK" > android/local.properties
"${TALIA_GRADLE:?Set TALIA_GRADLE to Gradle 8.13 executable}" -p android --no-daemon assembleDebug
