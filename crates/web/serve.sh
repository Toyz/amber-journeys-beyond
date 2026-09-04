#!/usr/bin/env bash
#
# Sets up and serves the streaming mode, for testing it without hosting
# anything permanently.
#
#   ./crates/web/serve.sh /path/to/extracted/disc
#
# Builds the wasm, writes a manifest, links the game's files into a scratch
# directory next to the page, and starts a static server. Nothing is copied:
# the disc is five hundred and seventy megabytes and stays where it is.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
disc=${1:-}
port=${2:-8080}

die() { printf '%s\n' "$*" >&2; exit 1; }

[ -n "$disc" ] || die "usage: $(basename "$0") <extracted disc> [port]

Point it at a directory holding ROXY/, MARGARET/, BRICE/ and EDWIN/ --
the game installed, or a mounted disc, or a copy of one.

To serve a disc image whole instead, put the .iso beside the page and open
  http://localhost:$port/?iso=<name>.iso
which needs no manifest and no this."

[ -d "$disc" ] || die "$disc is not a directory.

If it is an .iso, the streaming mode wants the files rather than the image;
mount it, or use the ?iso= mode described by $(basename "$0") with no
arguments."
disc=$(cd "$disc" && pwd)
[ -d "$disc/ROXY" ] || die "no ROXY/ in $disc -- is that the game?"

command -v wasm-pack >/dev/null || die "wasm-pack is not installed:
  cargo install wasm-pack"

echo "==> building the engine for the web"
wasm-pack build "$repo/crates/web" --target web \
    --out-dir page/pkg --out-name amber_web >/dev/null
printf '    %s of wasm\n' "$(du -h "$here/page/pkg/amber_web_bg.wasm" | cut -f1)"

# The page fetches `<base>/manifest.json` and then `<base>/<path>` for every
# path in it, so the manifest has to sit among the files it names. Linking the
# disc's top level rather than the disc itself keeps the manifest out of it --
# the disc may be read-only, and it is not ours to write into either way.
serve="$here/page/game"
echo "==> linking the disc into $(realpath --relative-to="$repo" "$serve")"
rm -rf "$serve"
mkdir -p "$serve"
for entry in "$disc"/*; do
    ln -s "$entry" "$serve/$(basename "$entry")"
done

echo "==> writing the manifest"
"$here/manifest.sh" "$disc" > "$serve/manifest.json"
files=$(grep -c '"' "$serve/manifest.json" || true)
first=$(cd "$disc" && du -ch $(find . -name '*.DAT') ROXY/ROXY.DXR 2>/dev/null | tail -1 | cut -f1)
total=$(du -sh "$disc" | cut -f1)
printf '    %s files, %s to the first frame, %s in all\n' "$files" "$first" "$total"

url="http://localhost:$port/?files=/game"
echo
echo "==> serving $(realpath --relative-to="$repo" "$here/page") on port $port"
echo
echo "    $url"
echo
echo "    Space  skip a film      S  dump the stage      C  cut content"
echo "    Ctrl-C to stop."
echo

# Python's own server is single threaded, which shows on a hundred megabyte
# chapter movie: the page cannot fetch the next file until this one is done.
# Anything threaded is better if it is here.
cd "$here/page"
if python3 - <<'PY' 2>/dev/null
import http.server, socketserver, sys
sys.exit(0 if hasattr(socketserver, "ThreadingTCPServer") else 1)
PY
then
    exec python3 - "$port" <<'PY'
import http.server, socketserver, sys

port = int(sys.argv[1])

class Handler(http.server.SimpleHTTPRequestHandler):
    # The wasm has to arrive as wasm or the browser will not stream-compile it,
    # and a mislabelled .mov is a film the page cannot decode.
    extensions_map = {
        **http.server.SimpleHTTPRequestHandler.extensions_map,
        ".wasm": "application/wasm",
        ".mov": "video/quicktime",
        ".json": "application/json",
    }

    def log_message(self, fmt, *args):
        # One line per file, without the timestamp noise.
        sys.stderr.write("    %s\n" % (fmt % args))


class Server(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True


with Server(("", port), Handler) as httpd:
    httpd.serve_forever()
PY
else
    exec python3 -m http.server "$port"
fi
