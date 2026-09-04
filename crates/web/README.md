# Amber in a browser

The engine names no platform, so this is only the three seams filled in: the
disc image is a `Content`, the canvas is a `Host`, and an audio worklet pulls
from the mixer. There is no game logic here.

## Build

```sh
wasm-pack build crates/web --target web --out-dir page/pkg --out-name amber_web
```

The build lands in `page/pkg`, which is not in the repo; `page` itself is the
hand-written half -- one HTML file and the audio worklet.

## Run

Any static server, from `crates/web/page`:

```sh
python3 -m http.server 8080
```

Then open `http://localhost:8080`.

## The disc

Three ways in, and the game is the same program in all three.

**The player's own image.** The default. A file picker, read straight into
memory, never uploaded. The ISO 9660 reader walks it exactly as it walks a
file on a desktop.

**One image, fetched.** `?iso=amber.iso` fetches an image sitting next to the
page instead of asking for one. Simple, and five hundred and seventy megabytes
before the first frame.

**Served as files.** `?files=/game` fetches a `manifest.json` from that path
and then the files it lists. The first load is the room data and Roxy's
chapter -- everything the engine needs to draw the first frame -- and the rest
arrives as the player reaches it. Margaret, Brice and Edwin are thirty to
forty megabytes each and are not touched until their chapter is entered.

The manifest is a JSON array of paths relative to the base:

```json
["ROXY/ROXY.DXR", "ROXY/ROXY_1.DAT", "ROXY/MOVIES/INTRO.MOV", ...]
```

which `find . -type f` and a little shaping will produce from an extracted
disc.

### How the streaming works

The engine reads synchronously and a browser fetches asynchronously. That is
the whole of the difficulty, and it is answered by one method on `Content`:

```rust
fn request(&self, path: &str) -> bool;
```

A read that misses says whether the file is coming. If it is, the engine holds
the film's wait until the bytes arrive rather than carrying on without them --
so a film that is still loading is waited for, not silently skipped. A
directory and an image answer `false` to everything, because they have it all
already, and behave exactly as they did.

Serving the game's data means distributing it, which is not ours to do. The
picker is the default for that reason.
