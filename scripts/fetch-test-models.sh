#!/usr/bin/env bash
# Downloads the models used by integration tests into $1 (default ./test-models),
# laid out exactly like the app's models dir: <root>/<slug>/...
set -euo pipefail
ROOT="${1:-$(pwd)/test-models}"
mkdir -p "$ROOT/.dl"
SHERPA=https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models

fetch() { # slug url sha256 kind(file|tar) filename
  local slug=$1 url=$2 sha=$3 kind=$4 name=$5
  local dest="$ROOT/.dl/$name"
  if [ -d "$ROOT/$slug" ]; then echo "have $slug"; return; fi
  echo "downloading $slug"
  curl -fL --retry 3 -C - -o "$dest" "$url"
  echo "$sha  $dest" | shasum -a 256 -c -
  mkdir -p "$ROOT/$slug"
  if [ "$kind" = tar ]; then tar xjf "$dest" -C "$ROOT/$slug"; rm "$dest"; else mv "$dest" "$ROOT/$slug/$name"; fi
}

fetch silero_vad "$SHERPA/silero_vad.onnx" 9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6 file silero_vad.onnx
fetch canary180m_flash "$SHERPA/sherpa-onnx-nemo-canary-180m-flash-en-es-de-fr-int8.tar.bz2" 7a38ed8b13f014ad632b09ff8d22e0c6f1359dd046af9235d281dfae841b9ab9 tar canary.tar.bz2
fetch parakeet_tdt_v2 "$SHERPA/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8.tar.bz2" 157c157bc51155e03e37d2466522a3a737dd9c72bb25f36eb18912964161e1ad tar parakeet-v2.tar.bz2
fetch qwen25_1_5b "https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF/resolve/91cad51170dc346986eccefdc2dd33a9da36ead9/qwen2.5-1.5b-instruct-q4_k_m.gguf" 6a1a2eb6d15622bf3c96857206351ba97e1af16c30d7a74ee38970e434e9407e file qwen2.5-1.5b-instruct-q4_k_m.gguf
echo "export DIKTATOR_TEST_MODELS=$ROOT"
