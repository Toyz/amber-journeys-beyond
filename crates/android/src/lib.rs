//! Amber on Android.
//!
//! The third front end, and the smallest, because the engine already had the
//! seams it needed:
//!
//!   - [`amber::content::Content`] is the disc, packed into the APK and read
//!     where it lies;
//!   - [`amber::host::Host`] is the surface below;
//!   - [`amber::audio::Sink`] is CPAL, which speaks AAudio here and is the same
//!     fifty lines the desktop uses.
//!
//! The loop is `amber::render::run`, shared with the desktop. Writing a second
//! one is how this engine shipped a dozen faults that were live in one front
//! end and invisible in the other, so there is not a second one.

use std::time::Duration;

use android_activity::input::{InputEvent, KeyAction, Keycode, MotionAction};
use android_activity::{AndroidApp, InputStatus, MainEvent, PollEvent};
use ndk::hardware_buffer_format::HardwareBufferFormat;

use amber::host::{Host, Input, Key};
use amber::iso::{Iso, ReadAt};

const STAGE_W: usize = 640;
const STAGE_H: usize = 480;

/// The disc, as it sits in the APK.
///
/// Android maps an uncompressed asset rather than reading it, so this costs no
/// resident memory: the pages the game touches are faulted in and the rest is
/// never read. A 574 MB disc behind a `ReadAt` is the same shape the desktop
/// gives `Iso` over a file, which is why the reader needed no changes.
struct Mapped(&'static [u8]);

impl ReadAt for Mapped {
    fn read_at(&self, at: u64, len: u64) -> Option<Vec<u8>> {
        let (at, len) = (at as usize, len as usize);
        self.0.get(at..at.checked_add(len)?).map(<[u8]>::to_vec)
    }
}

/// The surface, the touchscreen and the back key.
struct Android {
    app: AndroidApp,
    pointer: Option<(i32, i32)>,
    down: bool,

    open: bool,
}

impl Android {
    fn new(app: AndroidApp) -> Android {
        Android { app, pointer: None, down: false, open: true }
    }
}

impl Host for Android {
    fn poll(&mut self, _stage: (usize, usize)) -> Input {
        let app = self.app.clone();

        let mut closed = false;
        app.poll_events(Some(Duration::ZERO), |event| {
            if let PollEvent::Main(MainEvent::Destroy) = event {
                closed = true;
            }
        });
        if closed {
            self.open = false;
        }

        // The surface size is read once, so the mapping below is arithmetic
        // over locals and borrows nothing. The stage is always 640 by 480 and
        // letterboxed into whatever the device is -- the same sum the desktop
        // does for a resized window, and it lives in the host because the host
        // is the only thing that knows how it scaled.
        let surface = app.native_window().map(|w| (w.width() as f32, w.height() as f32));
        let (mut pointer, mut down) = (self.pointer, self.down);
        let mut pressed = Vec::new();

        if let (Some((sw, sh)), Ok(mut events)) = (surface, app.input_events_iter()) {
            let scale = (sw / STAGE_W as f32).min(sh / STAGE_H as f32);
            let ox = (sw - STAGE_W as f32 * scale) / 2.0;
            let oy = (sh - STAGE_H as f32 * scale) / 2.0;
            let to_stage = |x: f32, y: f32| {
                let (sx, sy) = (((x - ox) / scale) as i32, ((y - oy) / scale) as i32);
                (sx >= 0 && sy >= 0 && sx < STAGE_W as i32 && sy < STAGE_H as i32)
                    .then_some((sx, sy))
            };

            while events.next(|event| match event {
                InputEvent::MotionEvent(motion) => {
                    if let Some(p) = motion.pointers().next() {
                        let at = to_stage(p.x(), p.y());
                        match motion.action() {
                            // A touch is a press and a position at once. There
                            // is no hover on a phone, so the pointer goes where
                            // the finger is and the game's own cursor is drawn
                            // there.
                            MotionAction::Down | MotionAction::PointerDown => {
                                pointer = at;
                                down = true;
                            }
                            MotionAction::Move => pointer = at,
                            MotionAction::Up
                            | MotionAction::PointerUp
                            | MotionAction::Cancel => down = false,
                            _ => {}
                        }
                    }
                    InputStatus::Handled
                }
                // Back is the pause menu, which is where Quit lives -- so the
                // player is not trapped by it being claimed.
                InputEvent::KeyEvent(key)
                    if key.key_code() == Keycode::Back && key.action() == KeyAction::Up =>
                {
                    pressed.push(Key::Menu);
                    InputStatus::Handled
                }
                // Everything else belongs to the system. Claiming it takes the
                // volume keys away from the player, because `Handled` means the
                // framework never sees the press -- so the only events answered
                // for are the ones actually acted on here.
                _ => InputStatus::Unhandled,
            }) {}
        }

        self.pointer = pointer;
        self.down = down;
        Input { pointer, down, pressed, open: self.open, hover: false }
    }

    fn present(&mut self, frame: &[u32], stage: (usize, usize)) -> std::io::Result<()> {
        let Some(window) = self.app.native_window() else {
            // No surface yet, or the app is in the background. Not an error --
            // Android takes the window away whenever it likes and gives it
            // back, and the game carries on in between.
            return Ok(());
        };
        // The buffer is the surface's own size and the stage is drawn into
        // the middle of it. Asking for a 640 by 480 buffer instead would be
        // less code, but `ANativeWindow` scaling does not keep aspect -- it
        // stretches to fill, which on a 2.17:1 phone squashes a 4:3 game and,
        // worse, puts every touch somewhere other than where `to_stage`
        // thinks it is.
        let (sw, sh) = (window.width() as usize, window.height() as usize);
        let _ = window.set_buffers_geometry(
            sw as i32,
            sh as i32,
            Some(HardwareBufferFormat::R8G8B8X8_UNORM),
        );

        let Ok(mut buffer) = window.lock(None) else { return Ok(()) };
        let stride = buffer.stride();
        let (bw, bh) = (buffer.width(), buffer.height());
        let rows = bh;
        let bits = buffer.bits();

        // SAFETY: `bits` is the locked surface, which Android guarantees is
        // `stride * height` pixels of the format asked for above, and the lock
        // guard keeps it valid for as long as this borrow lasts. Nothing else
        // holds it: the guard is not `Clone` and does not leave this function.
        let out: &mut [u32] =
            unsafe { std::slice::from_raw_parts_mut(bits.cast::<u32>(), stride * rows) };

        // Nearest neighbour, which is the honest scaler for this material: the
        // plates are 256-colour dithered pre-renders and anything smoother
        // invents detail the disc never had.
        let scale = (bw as f32 / stage.0 as f32).min(bh as f32 / stage.1 as f32);
        let (dw, dh) = ((stage.0 as f32 * scale) as usize, (stage.1 as f32 * scale) as usize);
        let (ox, oy) = ((bw - dw) / 2, (bh - dh) / 2);

        for y in 0..bh {
            let row = &mut out[y * stride..y * stride + bw];
            if y < oy || y >= oy + dh {
                row.fill(0xff00_0000);
                continue;
            }
            let sy = ((y - oy) as f32 / scale) as usize;
            let src = &frame[sy.min(stage.1 - 1) * stage.0..][..stage.0];
            for (x, pixel) in row.iter_mut().enumerate() {
                if x < ox || x >= ox + dw {
                    *pixel = 0xff00_0000;
                    continue;
                }
                let sx = (((x - ox) as f32 / scale) as usize).min(stage.0 - 1);
                // The engine composes `0x00RRGGBB`. The surface wants bytes in
                // R, G, B, X order, which on a little-endian device reads back
                // as `0xXXBBGGRR` -- so red and blue swap.
                let composed = src[sx];
                let (r, g, b) = (composed >> 16 & 0xff, composed >> 8 & 0xff, composed & 0xff);
                *pixel = 0xff00_0000 | b << 16 | g << 8 | r;
            }
        }
        Ok(())
    }

    fn set_title(&mut self, _title: &str) {}
}

#[no_mangle]
fn android_main(app: AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Info),
    );

    // Wait for a surface before opening the game. The engine starts the
    // opening film the moment it has a world, and a film started against a
    // clock that has not run yet is instantly over -- which is exactly the
    // fault the browser build spent four rounds on.
    loop {
        if app.native_window().is_some() {
            break;
        }
        app.clone().poll_events(Some(Duration::from_millis(50)), |_| {});
    }

    // A game is watched as much as it is played -- the opening film alone is
    // ninety seconds of no touching -- so the screen must not go out under it.
    app.set_window_flags(
        android_activity::WindowManagerFlags::KEEP_SCREEN_ON,
        android_activity::WindowManagerFlags::empty(),
    );

    if let Err(e) = play(app) {
        log::error!("amber: {e}");
    }
}

fn play(app: AndroidApp) -> Result<(), Box<dyn std::error::Error>> {
    let manager = app.asset_manager();
    let name = std::ffi::CString::new("amber.iso")?;
    let asset = manager.open(&name).ok_or("no amber.iso in the APK")?;
    // Leaked so the mapping outlives the borrow: it is the disc, and it is
    // wanted for as long as the process is.
    let bytes: &'static [u8] = Box::leak(Box::new(asset)).buffer()?;
    log::info!("disc mapped, {} MB", bytes.len() / 1_048_576);

    let iso = Iso::over(Box::new(Mapped(bytes)))?;
    let mut game = amber::game::Game::from_content(Box::new(iso))?;
    log::info!("{} rooms", game.world.nodes.len());

    let audio = amber::audio::Audio::open();
    // The app's own directory: writable, needs no permission, and goes when
    // the app is uninstalled, which is the right lifetime for a save.
    let saves = app.internal_data_path().map(|dir| dir.join("amber.save"));
    log::info!("saves at {saves:?}");
    let mut host = Android::new(app);
    amber::render::run(
        &mut game,
        &mut host,
        audio,
        Vec::new(),
        false,
        1,
        amber::scale::Filter::default(),
        saves,
    )
}
