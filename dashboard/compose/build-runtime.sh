#!/usr/bin/env bash
# Builds the bounded QuickJS runtime for every ABI the Talìa shell declares.
set -euo pipefail
cd "$(dirname "$0")"
OUT="${1:?usage: build-runtime.sh OUTPUT_JNI_LIBS_DIR}"
SDK="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-$HOME/Android/Sdk}}"
NDK="${TALIA_NDK:-$SDK/ndk/27.0.12077973}"
TOOLCHAIN="$NDK/toolchains/llvm/prebuilt/linux-x86_64"
[[ -d "$TOOLCHAIN" ]] || { echo "Missing NDK toolchain: $TOOLCHAIN" >&2; exit 1; }
for pair in arm64-v8a:aarch64-linux-android:aarch64-linux-android armeabi-v7a:armv7-linux-androideabi:armv7a-linux-androideabi \
            x86:i686-linux-android:i686-linux-android x86_64:x86_64-linux-android:x86_64-linux-android; do
 IFS=: read -r abi target clang <<<"$pair"
 env_target="${target//-/_}"
 env "CC_${env_target}=$TOOLCHAIN/bin/${clang}26-clang" "AR_${env_target}=$TOOLCHAIN/bin/llvm-ar" \
  "CARGO_TARGET_${env_target^^}_LINKER=$TOOLCHAIN/bin/${clang}26-clang" \
  "BINDGEN_EXTRA_CLANG_ARGS=--sysroot=$TOOLCHAIN/sysroot --target=${clang}26" \
  cargo build --quiet --release --manifest-path ../native/Cargo.toml --target "$target" --locked --offline
 mkdir -p "$OUT/$abi"
 cp "../native/target/$target/release/libtalia_dashboard_runtime.so" "$OUT/$abi/"
done
