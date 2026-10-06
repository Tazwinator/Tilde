#!/usr/bin/env bash
# Downloads the offline text-to-speech engine (Piper) and a Castilian
# Spanish voice into ~/.local/share/tilde/tts. Fully local afterwards.
#
# Every download is pinned (a Piper release tag, a piper-voices commit) and
# checked against its sha256, and only moved into place once it checks out,
# so a failed or tampered download never leaves a half-installed engine.
# Without Piper, Tilde falls back to espeak-ng if it is installed.
set -euo pipefail

BASE="${HOME}/.local/share/tilde/tts"
PIPER_RELEASE="https://github.com/rhasspy/piper/releases/download/2023.11.14-2"
VOICES_REV="c10ece1aade47bb51c153c893d14e5bf8e5b7117"
VOICE_URL="https://huggingface.co/rhasspy/piper-voices/resolve/${VOICES_REV}/es/es_ES/davefx/medium"
VOICE="es_ES-davefx-medium"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)   PIPER_PKG=piper_linux_x86_64.tar.gz;  PIPER_SHA=a50cb45f355b7af1f6d758c1b360717877ba0a398cc8cbe6d2a7a3a26e225992 ;;
  Linux-aarch64)  PIPER_PKG=piper_linux_aarch64.tar.gz; PIPER_SHA=fea0fd2d87c54dbc7078d0f878289f404bd4d6eea6e7444a77835d1537ab88eb ;;
  Linux-armv7l)   PIPER_PKG=piper_linux_armv7l.tar.gz;  PIPER_SHA=c6946fcd57c705ed1d4666ea880f80ba0bbbd14de62ecbdd13460baf3bac8e37 ;;
  Darwin-x86_64)  PIPER_PKG=piper_macos_x64.tar.gz;     PIPER_SHA=ced85c0a3df13945b1e623b878a48fdc2854d5c485b4b67f62857cf551deaf8b ;;
  Darwin-arm64)   PIPER_PKG=piper_macos_aarch64.tar.gz; PIPER_SHA=6b1eb03b3735946cb35216e063e7eebcc33a6bbf5dd96ec0217959bf1cdcb0cc ;;
  *)
    echo "No Piper build for $(uname -s) $(uname -m)." >&2
    echo "Install espeak-ng from your package manager instead; Tilde uses it automatically." >&2
    exit 1
    ;;
esac
MODEL_SHA=6658b03b1a6c316ee4c265a9896abc1393353c2d9e1bca7d66c2c442e222a917
CONFIG_SHA=0e0dda87c732f6f38771ff274a6380d9252f327dca77aa2963d5fbdf9ec54842

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

sha256() {
  if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

# fetch URL SHA256 DEST: download to the temp dir, verify, then move into place.
fetch() {
  local part
  part="$TMP/$(basename "$3").part"
  curl -fsSL --retry 3 -o "$part" "$1"
  if [ "$(sha256 "$part")" != "$2" ]; then
    echo "Checksum mismatch for $1; not installing it." >&2
    exit 1
  fi
  mv "$part" "$3"
}

mkdir -p "$BASE/voice"

if [ ! -x "$BASE/piper/piper" ]; then
  echo "Downloading Piper engine..."
  fetch "$PIPER_RELEASE/$PIPER_PKG" "$PIPER_SHA" "$TMP/piper.tar.gz"
  mkdir -p "$TMP/unpacked"
  tar -xzf "$TMP/piper.tar.gz" -C "$TMP/unpacked"
  rm -rf "$BASE/piper"
  mv "$TMP/unpacked/piper" "$BASE/piper"
else
  echo "Piper engine already present."
fi

if [ ! -f "$BASE/voice/$VOICE.onnx" ] || [ "$(sha256 "$BASE/voice/$VOICE.onnx")" != "$MODEL_SHA" ]; then
  echo "Downloading Castilian voice (es_ES davefx, ~60MB)..."
  fetch "$VOICE_URL/$VOICE.onnx.json" "$CONFIG_SHA" "$BASE/voice/$VOICE.onnx.json"
  fetch "$VOICE_URL/$VOICE.onnx" "$MODEL_SHA" "$BASE/voice/$VOICE.onnx"
else
  echo "Voice already present."
fi

echo "Done. TTS will be picked up next time Tilde starts."
