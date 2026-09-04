#!/usr/bin/env bash
# Builds and packages the APK.
#
# No Gradle: the Android SDK ships everything this needs, and a build file that
# does four things is easier to be sure of than one that does four hundred.
#
#   AMBER_ISO=/path/to/amber.iso ./pack.sh
#
# The disc is stored rather than deflated, which is the one detail that
# matters: Android maps an uncompressed asset, so the game reads the pages it
# touches and nothing else. Compressed, it would have to be unpacked to disk
# first and the app would need 574 MB of storage it has no business asking for.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
: "${ANDROID_HOME:=$HOME/Android/Sdk}"
build_tools="$(ls -d "$ANDROID_HOME"/build-tools/* | sort -V | tail -1)"
platform="$(ls -d "$ANDROID_HOME"/platforms/android-* | sort -V | tail -1)"
abi="${ABI:-arm64-v8a}"
out="$here/build"

echo "sdk         $ANDROID_HOME"
echo "build-tools $(basename "$build_tools")"
echo "platform    $(basename "$platform")"
echo "abi         $abi"

rm -rf "$out" && mkdir -p "$out/lib/$abi" "$out/assets"

# 1. The engine, as a shared library. build.rs refuses without a disc.
# API 26 is the floor: CPAL links AAudio, which does not exist below it.
( cd "$root" && cargo ndk -t "$abi" --platform 26 -o "$out/lib" build --release -p amber-android )
ls -lh "$out/lib/$abi/"

# 2. The disc, as it was copied in by build.rs.
cp "$here/assets/amber.iso" "$out/assets/amber.iso"

# 3. The launcher icon, taken off the disc rather than drawn: A_JB.ICO is the
#    icon the game shipped with in 1996. It is 32 by 32 at its largest, so it
#    is grown with a point filter -- the same argument as the upscaler, that
#    smoothing invents detail the original never had.
xorriso -osirrox on -indev "$here/assets/amber.iso" \
  -extract /A_JB.ICO "$out/icon.ico" >/dev/null 2>&1
for d in mdpi:48 hdpi:72 xhdpi:96 xxhdpi:144 xxxhdpi:192; do
  mkdir -p "$out/res/mipmap-${d%%:*}"
  magick "$out/icon.ico[3]" -filter point -resize "${d##*:}x${d##*:}" \
    "$out/res/mipmap-${d%%:*}/ic_launcher.png"
done

# 4. Manifest and resources.
"$build_tools/aapt2" compile --dir "$out/res" -o "$out/res.zip" >/dev/null
"$build_tools/aapt2" link \
  --manifest "$here/AndroidManifest.xml" \
  -I "$platform/android.jar" \
  -R "$out/res.zip" --auto-add-overlay \
  --min-sdk-version 26 --target-sdk-version 34 \
  -o "$out/base.apk"

# 5. Everything else, added as stored entries. `zip -0` is the whole trick.
( cd "$out" && zip -q -0 -r base.apk lib assets )

# 6. Align, then sign. Alignment has to come first: signing is over the final
#    bytes, and zipalign rewrites offsets.
"$build_tools/zipalign" -p -f 4 "$out/base.apk" "$out/amber-aligned.apk"

keystore="$here/debug.keystore"
if [ ! -f "$keystore" ]; then
  keytool -genkeypair -keystore "$keystore" -alias amber -storepass android \
    -keypass android -keyalg RSA -keysize 2048 -validity 10000 \
    -dname "CN=Amber debug" >/dev/null 2>&1
fi
"$build_tools/apksigner" sign --ks "$keystore" --ks-pass pass:android \
  --key-pass pass:android --out "$here/amber.apk" "$out/amber-aligned.apk"

rm -rf "$out"
ls -lh "$here/amber.apk"
echo
echo "install with:  adb install -r $here/amber.apk"
