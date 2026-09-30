#!/usr/bin/env bash
# Downloads the offline text-to-speech engine (Piper) and a Castilian
# Spanish voice into ~/.local/share/tilde/tts. Fully local afterwards.
set -euo pipefail

BASE="${HOME}/.local/share/tilde/tts"
PIPER_URL="https://github.com/rhasspy/piper/releases/download/2023.11.14-2/piper_linux_x86_64.tar.gz"
VOICE_URL="https://huggingface.co/rhasspy/piper-voices/resolve/main/es/es_ES/davefx/medium"

mkdir -p "$BASE/voice"

if [ ! -x "$BASE/piper/piper" ]; then
  echo "Downloading Piper engine..."
  curl -sL --retry 3 -o /tmp/piper.tar.gz "$PIPER_URL"
  tar -xzf /tmp/piper.tar.gz -C "$BASE"
  rm -f /tmp/piper.tar.gz
else
  echo "Piper engine already present."
fi

if [ ! -f "$BASE/voice/es_ES-davefx-medium.onnx" ]; then
  echo "Downloading Castilian voice (es_ES davefx, ~60MB)..."
  curl -sL --retry 3 -o "$BASE/voice/es_ES-davefx-medium.onnx" "$VOICE_URL/es_ES-davefx-medium.onnx"
  curl -sL --retry 3 -o "$BASE/voice/es_ES-davefx-medium.onnx.json" "$VOICE_URL/es_ES-davefx-medium.onnx.json"
else
  echo "Voice already present."
fi

echo "Done. TTS will be picked up next time Tilde starts."
