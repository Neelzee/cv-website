#!/bin/sh

set -eu
VERSION=2.0.4
SHA256=e209dda5c8235479f3166defc7750e1dbcd5a5c1808b7792fc2e6733768fb447

cd "$(dirname "$0")/.."
OUT="static/htmx-$VERSION.min.js"
[ -f "$OUT" ] && echo "$SHA256  $OUT" | sha256sum -c --quiet - 2>/dev/null && exit 0

curl -fsSL "https://unpkg.com/htmx.org@$VERSION/dist/htmx.min.js" -o "$OUT.tmp"
echo "$SHA256  $OUT.tmp" | sha256sum -c --quiet - || { rm -f "$OUT.tmp"; echo "htmx checksum mismatch" >&2; exit 1; }
mv "$OUT.tmp" "$OUT"
