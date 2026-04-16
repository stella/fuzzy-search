#!/bin/bash
# Download benchmark corpora.
#
# Canterbury Large Corpus (ASCII, academic):
#   https://corpus.canterbury.ac.nz/
#
# Leipzig Corpora Collection (multilingual, academic):
#   https://wortschatz.uni-leipzig.de/en/download/
#
# Ref: D. Goldhahn, T. Eckart, U. Quasthoff.
# "Building Large Monolingual Dictionaries at the
# Leipzig Corpora Collection." LREC 2012.

set -euo pipefail
cd "$(dirname "$0")"
mkdir -p corpus
cd corpus

echo "=== Canterbury Large Corpus ==="
if [ ! -f bible.txt ]; then
  curl -Lo large.zip \
    "https://corpus.canterbury.ac.nz/resources/large.zip"
  unzip -o large.zip
  rm large.zip
  echo "Done: bible.txt, world192.txt, E.coli"
else
  echo "Already present, skipping."
fi

echo ""
echo "=== Leipzig Corpora Collection ==="

CORPORA=(
  "ces_news_2024_300K"
  "deu_news_2024_300K"
)

BASE="https://downloads.wortschatz-leipzig.de/corpora"

for name in "${CORPORA[@]}"; do
  if [ -f "${name}.txt" ]; then
    echo "${name}.txt already present, skipping."
    continue
  fi
  echo "Downloading ${name}..."
  curl -sLO "${BASE}/${name}.tar.gz"
  tar xzf "${name}.tar.gz" \
    "${name}/${name}-sentences.txt"
  NAME="${name}" python3 - <<'PY'
import os
from pathlib import Path

name = os.environ["NAME"]
source = Path(name) / f"{name}-sentences.txt"
target = Path(f"{name}.txt")

with source.open("r", encoding="utf-8", errors="ignore") as src, target.open(
    "w", encoding="utf-8"
) as dst:
    for index, line in enumerate(src):
        parts = line.rstrip("\n").split("\t", 1)
        if len(parts) == 2:
            dst.write(parts[1] + "\n")
        if index + 1 >= 50000:
            break
PY
  rm -rf "${name}" "${name}.tar.gz"
  SIZE=$(wc -c < "${name}.txt" | tr -d ' ')
  echo "  ${name}.txt: ${SIZE} bytes"
done

echo ""
echo "Done. All corpora ready."
