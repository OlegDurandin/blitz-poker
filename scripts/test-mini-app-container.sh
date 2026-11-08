#!/usr/bin/env bash
set -euo pipefail
image=${1:?pass image tag}
cid=$(docker run -d --read-only --tmpfs /tmp:rw,size=16m,mode=1777 --cap-drop ALL --security-opt no-new-privileges -p 127.0.0.1::8080 "$image")
work=$(mktemp -d)
trap 'docker rm -f "$cid" >/dev/null; rm -rf "$work"' EXIT
port=$(docker port "$cid" 8080/tcp | head -1 | awk -F: '{print $NF}')
url="http://127.0.0.1:$port"
for i in {1..30}; do
  if curl -fsS "$url/healthz" > /dev/null; then break; fi
  sleep 1
done
curl -fsS "$url/healthz" | grep -q ok
curl -fsS -D "$work/headers" "$url/" > "$work/index.html"
grep -qi 'Cache-Control: no-cache' "$work/headers"
python3 - "$work/index.html" > "$work/assets" <<'ASSETS'
import re, sys
from pathlib import Path
assets=re.findall(r'(?:src|href)="(\.?/assets/[^"?#]+)"', Path(sys.argv[1]).read_text())
assert assets, 'no bundled assets in index'
print('\n'.join(a.removeprefix('./').lstrip('/') for a in assets))
ASSETS
while IFS= read -r asset; do
  curl -fsS -D "$work/headers" "$url/$asset" -o "$work/asset"
  grep -qi 'Cache-Control: public, max-age=31536000, immutable' "$work/headers"
done < "$work/assets"
wasm=$(docker exec "$cid" find /usr/share/nginx/html/assets -name '*.wasm' | head -1)
test -n "$wasm"
curl -fsS "$url/assets/${wasm##*/}" -o "$work/engine.wasm"
python3 - "$work/engine.wasm" <<'WASM'
import sys
from pathlib import Path
assert Path(sys.argv[1]).read_bytes()[:4] == b'\x00asm'
WASM
test "$(curl -s -o /dev/null -w '%{http_code}' "$url/not-found.js")" = 404
test "$(curl -s -o /dev/null -w '%{http_code}' "$url/.env")" = 403
test "$(docker exec "$cid" id -u)" != 0
if docker exec "$cid" touch /usr/share/nginx/html/unexpected; then
  echo 'root filesystem is writable' >&2; exit 1
fi
echo 'Mini App container smoke test passed'
