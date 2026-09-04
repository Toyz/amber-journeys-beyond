//! The disc is a build input, not something the app goes looking for at run
//! time.
//!
//! An Android build has nowhere sensible to ask a player for a 574 MB file, so
//! the disc is packed into the APK and the build fails without one. That is
//! deliberate: an APK that installs and then cannot find its game is a worse
//! failure than one that never builds, because the first is only discovered on
//! a phone.
//!
//! The image itself is never committed -- `*.iso` is in `.gitignore`, and this
//! copies whatever `AMBER_ISO` names into `assets/` at build time.

use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=AMBER_ISO");

    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
    let packed = assets.join("amber.iso");

    if let Some(from) = std::env::var_os("AMBER_ISO").map(PathBuf::from) {
        if !from.is_file() {
            panic!("AMBER_ISO is set to {} and that is not a file", from.display());
        }
        std::fs::create_dir_all(&assets).expect("could not make assets/");
        // Copying rather than symlinking: the APK packer follows the tree it
        // is given, and a dangling link inside it produces an APK with a
        // zero-length disc in it that installs perfectly well.
        std::fs::copy(&from, &packed).expect("could not copy the disc into assets/");
        println!("cargo:rerun-if-changed={}", from.display());
        return;
    }

    if packed.is_file() {
        println!("cargo:rerun-if-changed={}", packed.display());
        return;
    }

    panic!(
        "\n\n\
         No game disc to build with.\n\n\
         The engine ships no game content, so the APK has to be told where a\n\
         disc is. Point AMBER_ISO at one:\n\n\
         \x20   AMBER_ISO=/path/to/amber.iso cargo ndk -t arm64-v8a build --release\n\n\
         An image made from an installed copy works too:\n\n\
         \x20   genisoimage -o amber.iso -R -J extract/\n\n"
    );
}
