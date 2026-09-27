#!/usr/bin/env bash
# Build/select Talìa artifacts; delegate validation, authentication and uploads.
set -euo pipefail
SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
REPOSITORY_DIR=$(cd -- "$SCRIPT_DIR/.." && pwd)
ANDROID_DIR="${TALIA_ANDROID_DIR:-$REPOSITORY_DIR/android}"
# Included source builds also need the SDK location.
export ANDROID_HOME="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Android/Sdk}}"
export LELLOSTORE_URL="${LELLOSTORE_URL:-https://store.lelloman.com}"
export LELLOSTORE_OIDC_ISSUER="${LELLOSTORE_OIDC_ISSUER:-https://auth.lelloman.com}"
export LELLOSTORE_CLIENT_ID="${LELLOSTORE_CLIENT_ID:-22cd4a2d-a771-41e3-b76e-3f83ff8e9bbf}"

fail() { echo "$*" >&2; exit 1; }
usage() {
    cat <<'HELP'
Usage: publish-android-to-lellostore.sh [wrapper options] [publisher options]

Default: build :app:assembleParavoidAndroidRelease in TALIA_ANDROID_DIR
(default ../android relative to this script), then upload its shell APK draft.

  --artifact PATH       Use an existing signed shell APK; do not build
  --payload PATH        Upload an existing signed VPK instead of a shell
  --package-name NAME   Required with --payload; exact installed package identity
  --contract-id ID      Required with --payload; accepted shell contract ID
  --build-only          Build the shell and embedded payload without uploading
  --help                Show this help

Publisher options are forwarded, e.g. --dry-run --json, --beta, --store-url URL.
--dry-run validates a shell locally without authentication/upload. The shared
publisher does not support VPK dry-runs. --yes must be supplied explicitly for
an already authorized noninteractive upload. Upload creates a draft; publishing
is a separate operation. Versions are never incremented automatically.
HELP
}
artifact=''; payload=''; package=''; contract=''; build_only=false; dry_run=false
forward=()
while (($#)); do
    case "$1" in
        --help|-h) usage; exit 0 ;;
        --artifact|--payload|--package-name|--contract-id)
            (($# >= 2)) && [[ -n "$2" && "$2" != --* ]] || fail "Missing value for $1"
            case "$1" in
                --artifact) artifact=$2 ;;
                --payload) payload=$2 ;;
                --package-name) package=$2 ;;
                --contract-id) contract=$2 ;;
            esac
            shift 2 ;;
        --build-only) build_only=true; shift ;;
        --dry-run) dry_run=true; forward+=("$1"); shift ;;
        --distribution-mode|--distribution-mode=*) fail 'This wrapper uploads Paravoid shells only; distribution mode is fixed.' ;;
        --replace-latest) fail 'Replacing existing versions is not supported by this wrapper.' ;;
        *) forward+=("$1"); shift ;;
    esac
done
[[ -z "$artifact" || -z "$payload" ]] || fail 'Choose --artifact or --payload, not both.'
if [[ "$build_only" == true ]]; then
    [[ -z "$artifact$payload" && ${#forward[@]} == 0 ]] || fail '--build-only cannot be combined with prebuilt artifacts or publisher options.'
fi
if [[ -n "$payload" ]]; then
    [[ -n "$package" && -n "$contract" ]] || fail '--payload requires --package-name and --contract-id.'
    [[ "$payload" == *.vpk && -s "$payload" ]] || fail "Expected a non-empty .vpk: $payload"
    [[ "$dry_run" == false ]] || fail 'The shared publisher does not support --dry-run for VPKs; no upload attempted.'
else
    [[ -z "$package$contract" ]] || fail '--package-name and --contract-id are only valid with --payload.'
fi
if [[ "$build_only" == false ]]; then
    PUBLISHER="${LELLOSTORE_PUBLISHER:-}"
    if [[ -z "$PUBLISHER" && -x "$REPOSITORY_DIR/../lellostore/scripts/publish-to-lellostore.py" ]]; then
        PUBLISHER="$REPOSITORY_DIR/../lellostore/scripts/publish-to-lellostore.py"
    fi
    PUBLISHER="${PUBLISHER:-$HOME/lelloprojects/lellostore/scripts/publish-to-lellostore.py}"
    [[ -x "$PUBLISHER" ]] || fail 'Set LELLOSTORE_PUBLISHER to the executable shared LelloStore publisher.'
fi
if [[ -n "$payload" ]]; then
    exec "$PUBLISHER" upload-vpk "$package" "$contract" "$payload" "${forward[@]}"
fi
if [[ -z "$artifact" ]]; then
    [[ -x "$ANDROID_DIR/gradlew" ]] || fail "Production Android build is not configured at $ANDROID_DIR. Set TALIA_ANDROID_DIR or use --artifact; runtime-spike APKs are not selected automatically."
    [[ -s "$ANDROID_DIR/signing.properties" ]] || fail "Missing release signing configuration: $ANDROID_DIR/signing.properties"
    export PARAVOID_SIGNING_KEY="${PARAVOID_SIGNING_KEY:-$HOME/.config/talia/paravoid-release/talia-release.pk8}"
    export PARAVOID_SIGNING_KEY_ID="${PARAVOID_SIGNING_KEY_ID:-talia-release}"
    export PARAVOID_TRUST_POLICY="${PARAVOID_TRUST_POLICY:-$HOME/.config/talia/paravoid-release/trust.json}"
    export PARAVOID_UPDATE_BASE_URL="${PARAVOID_UPDATE_BASE_URL:-https://store.lelloman.com/api/paravoid/}"
    [[ -s "$PARAVOID_SIGNING_KEY" && -s "$PARAVOID_TRUST_POLICY" ]] || fail 'Missing Talìa Paravoid signing key or trust policy.'
    artifact="$ANDROID_DIR/app/build/outputs/paravoid/paravoidAndroidRelease/shell.apk"
    embedded="$ANDROID_DIR/app/build/outputs/paravoid/paravoidAndroidRelease/payload.vpk"
    (cd -- "$ANDROID_DIR" && ./gradlew :app:assembleParavoidAndroidRelease)
    [[ -s "$embedded" ]] || fail "Build did not produce the embedded payload: $embedded"
fi
[[ "$artifact" == *.apk && -s "$artifact" ]] || fail "Expected a non-empty signed shell .apk: $artifact"
echo "Paravoid release shell: $artifact ($(stat --format='%s' "$artifact") bytes)" >&2
if [[ "$build_only" == true ]]; then exit 0; fi
exec "$PUBLISHER" upload "$artifact" "${forward[@]}" --distribution-mode paravoid
