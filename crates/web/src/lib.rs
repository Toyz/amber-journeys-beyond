//! *Amber: Journeys Beyond*, in a browser.
//!
//! The engine names no platform, so this is only the three seams filled in:
//! the image the player picked is a [`Content`], the canvas is where a frame
//! goes, and the audio worklet pulls samples out of the mixer. There is no
//! game logic here and there must not be -- anything that had to be written
//! twice would be a hole in the traits rather than a thing to solve here.
//!
//! The player supplies their own disc. Nothing is hosted and nothing is
//! fetched: the file goes straight from the picker into memory, and the ISO
//! 9660 reader walks it exactly as it walks a file on a desktop.

mod streamed;

use amber::content::Content;
use streamed::Streamed;
use amber::game::Game;
use amber::host::Key;
use amber::iso::Iso;
use wasm_bindgen::prelude::*;
use wasm_bindgen::Clamped;

/// The key a film's soundtrack is mixed under, so it can be stopped when its
/// picture goes. Without one it outlived the room it belonged to.
const FILM: &str = "\u{1}filmSoundtrack";

const STAGE_W: usize = 640;
const STAGE_H: usize = 480;

/// The game, and everything the page needs to drive it.
#[wasm_bindgen]
pub struct Amber {
    game: Game,
    audio: Option<amber::audio::Audio>,
    /// The composed scene, redrawn only when something changes.
    frame: Vec<u32>,
    /// The scene plus the cursor, which moves every frame.
    out: Vec<u32>,
    /// `out` again as RGBA bytes, which is what a canvas wants.
    rgba: Vec<u8>,
    dirty: bool,
    pointer: Option<(i32, i32)>,
    down: bool,
    was_down: bool,
    hot: bool,
    /// Set when the game is being streamed, so the page knows to keep feeding
    /// it. `None` when the whole disc image is already in memory.
    streamed: Option<Streamed>,
    /// Whether the film that is playing has had its soundtrack handed over.
    soundtrack_started: bool,
    /// The room the mixer's ambient loops were set for.
    ambience_room: usize,
    /// Whether the dither is taken out before the frame is shown.
    undither: bool,
    /// What the film channel last showed, so a film reopening on its own is
    /// visible in the console rather than only on screen.
    watching: Option<(String, bool)>,
    title: String,
}

#[wasm_bindgen]
impl Amber {
    /// Opens a disc image the page has read into memory.
    ///
    /// Five hundred and seventy megabytes of it, which a desktop browser holds
    /// without complaint. Every read after this is a slice of that array, so
    /// nothing is asynchronous once the game is running -- which is what lets
    /// the engine stay exactly as it is.
    #[wasm_bindgen(constructor)]
    pub fn new(image: Vec<u8>) -> Result<Amber, JsValue> {
        let iso = Iso::over(Box::new(image))
            .map_err(|e| JsValue::from_str(&format!("not a disc image: {e}")))?;
        log(&format!("{} files on the image", iso.count()));
        Amber::over(Box::new(iso) as Box<dyn Content>)
    }

    fn over(content: Box<dyn Content>) -> Result<Amber, JsValue> {
        let game = Game::from_content(content).map_err(|e| JsValue::from_str(&e.to_string()))?;
        log(&format!("{} rooms", game.world.count()));
        Ok(Amber {
            game,
            audio: None,
            frame: vec![0; STAGE_W * STAGE_H],
            out: vec![0; STAGE_W * STAGE_H],
            rgba: vec![0; STAGE_W * STAGE_H * 4],
            dirty: true,
            pointer: None,
            down: false,
            was_down: false,
            hot: false,
            streamed: None,
            soundtrack_started: false,
            ambience_room: usize::MAX,
            undither: false,
            watching: None,
            title: String::new(),
        })
    }

    /// Opens the game over files served one at a time.
    ///
    /// `manifest` is every path the server will serve and `seed` is what the
    /// page has already fetched -- the room data and the chapter the player
    /// starts in, which is all the engine needs to draw the first frame.
    /// Everything else is asked for as it is reached.
    pub fn streaming(manifest: Vec<JsValue>, seed: js_sys::Object) -> Result<Amber, JsValue> {
        let paths: Vec<String> = manifest.iter().filter_map(|p| p.as_string()).collect();
        let store = Streamed::with_manifest(paths);
        for entry in js_sys::Object::entries(&seed).iter() {
            let pair = js_sys::Array::from(&entry);
            let Some(path) = pair.get(0).as_string() else { continue };
            let bytes = js_sys::Uint8Array::new(&pair.get(1)).to_vec();
            store.put(&path, bytes);
        }
        let mut game = Amber::over(Box::new(store.clone()))?;
        game.streamed = Some(store);
        Ok(game)
    }

    /// What the engine has asked for and not been given, for the page to go
    /// and fetch. Empty unless the game is being streamed.
    pub fn wanted(&self) -> Vec<JsValue> {
        match &self.streamed {
            Some(s) => s.take_wanted().into_iter().map(JsValue::from).collect(),
            None => Vec::new(),
        }
    }

    /// Hands over a file the page has fetched.
    pub fn supply(&mut self, path: &str, bytes: Vec<u8>) {
        if let Some(s) = &self.streamed {
            s.put(path, bytes);
        }
    }

    /// Whether the game is holding for something that has not arrived, so the
    /// page can say so rather than looking frozen.
    pub fn waiting(&self) -> bool {
        self.game.awaiting_content()
    }

    /// Takes the ordered dither out of the picture before it is shown.
    ///
    /// The scaling itself is the browser's -- the canvas is 640 by 480 and CSS
    /// decides how big that is drawn -- so the only part worth doing here is
    /// the part CSS cannot: the dither is a lossy encoding of a smoother
    /// original, and taking it out at native size lets the browser's own
    /// interpolation work on what was underneath rather than on the dots.
    pub fn set_undither(&mut self, on: bool) {
        self.undither = on;
        self.dirty = true;
    }

    /// Turns the engine's own tracing on and sends it to the console.
    ///
    /// `topics` is what the desktop takes in `AMBER_TRACE` -- `video`,
    /// `audio`, `script`, `room`, `sprite`, `state`, or `all`.
    pub fn trace(&self, topics: &str) {
        amber::trace::listen(topics, log);
    }

    /// Starts the mixer at the rate the page's audio context runs at.
    ///
    /// The sink has nothing to hold: the worklet pulls through [`Amber::fill`]
    /// rather than being pushed to, so this only sizes the mixer.
    pub fn start_audio(&mut self, rate: u32, channels: u16) {
        struct Pulled;
        impl amber::audio::Sink for Pulled {}
        self.audio = amber::audio::Audio::over(rate, channels, |_| {
            Some(Box::new(Pulled) as Box<dyn amber::audio::Sink>)
        });
    }

    /// Mixes the next samples, interleaved. Called by the audio worklet.
    pub fn fill(&mut self, out: &mut [f32]) {
        match &self.audio {
            Some(audio) => audio.fill(out),
            None => out.fill(0.0),
        }
    }

    /// Where the pointer is, in stage coordinates. The page maps them, because
    /// the page is what decided how big to draw the canvas.
    pub fn pointer(&mut self, x: i32, y: i32, inside: bool) {
        self.pointer = inside.then_some((x, y));
        let over_bar = self
            .pointer
            .is_some_and(|(_, y)| y > amber::inventory::Inventory::top_y(STAGE_H as i32));
        if over_bar != self.hot {
            self.hot = over_bar;
            self.dirty = true;
        }
    }

    pub fn button(&mut self, down: bool) {
        self.down = down;
    }

    /// One of the four keys the engine knows, by name.
    pub fn key(&mut self, name: &str) {
        let key = match name {
            "skip" => Key::Space,
            "hotspots" => Key::Hotspots,
            "stage" => Key::Stage,
            "cut" => Key::Cut,
            _ => return,
        };
        match key {
            Key::Space => {
                if self.game.skip_video() {
                    // The film's soundtrack is a one-shot in the mixer and
                    // knows nothing about the picture being cut short, so
                    // skipping the opening left two minutes of it playing over
                    // a silent house.
                    if let Some(audio) = &self.audio {
                        audio.stop_oneshots();
                    }
                    self.soundtrack_started = false;
                    self.dirty = true;
                }
            }
            Key::Stage => {
                for line in self.game.stage_report() {
                    log(&line);
                }
            }
            Key::Cut => self.show_cut(),
            Key::Escape | Key::Hotspots => {}
        }
    }

    fn show_cut(&mut self) {
        let domain = self.game.node().domain.clone();
        let carried = amber::natives::cut::in_chapter(&domain);
        let Some(cut) = carried.first() else {
            log(&format!("nothing cut in {domain}"));
            return;
        };
        log(&format!("{}: {}", cut.name, cut.about));
        let mut out = amber::script::Outcome::default();
        amber::natives::cut::call(cut.name, &[], &mut self.game.state, &mut out);
        if out.effects.is_empty() {
            if let Some(needs) = cut.needs {
                log(&format!("   nothing to show: it wants {needs}"));
            }
        }
        self.game.play_outcome(out);
        self.dirty = true;
    }

    /// The room the player is in, for the page to put in the title bar.
    pub fn title(&mut self) -> String {
        let room = self.game.node();
        format!(
            "{} / {}",
            room.domain,
            room.name.clone().unwrap_or_default()
        )
    }

    /// Advances the game by one frame and paints the canvas.
    ///
    /// `now` is `performance.now()` in milliseconds: the engine has no clock
    /// of its own on this platform, so the page supplies one.
    pub fn frame(&mut self, now: f64, ctx: &web_sys::CanvasRenderingContext2d) -> Result<(), JsValue> {
        amber::clock::advance(now / 1000.0);

        // A sequence holds part way through, so the queue is pumped every
        // frame and not only in the frame a click arrives.
        if self.game.effects_busy() {
            for effect in self.game.drain_ready() {
                self.apply(effect);
            }
        }
        if self.game.script_running() {
            let outcome = self.game.pump();
            if !outcome.effects.is_empty() || outcome.redraw || outcome.destination.is_some() {
                self.dirty = true;
            }
        }
        for effect in self.game.due_cues() {
            self.apply(effect);
        }
        if self.game.poll_content() {
            self.dirty = true;
        }
        // The room's ambience is the room's, and it has to be re-levelled when
        // the room changes -- loops the new one does not want are retired and
        // the ones it does are set to the level it asks for. Without it every
        // room's bed stacked on the last, which is what was looping.
        if self.game.room != self.ambience_room {
            self.ambience_room = self.game.room;
            // A film belongs to the room that placed it, and so does its
            // sound: walking away took the picture and left the soundtrack.
            if let Some(audio) = &self.audio {
                audio.stop(FILM);
            }
            self.soundtrack_started = false;
            amber::game::update_ambience(&mut self.game, self.audio.as_ref());
        }
        if self.game.tick_overlay() {
            self.dirty = true;
        }
        // Says so when the film on screen changes, or when the same one is
        // opened again -- a film that restarts itself is the hardest kind of
        // fault to see from the outside, and this is the only place a browser
        // can be asked what it thinks it is doing.
        let showing = self
            .game
            .player
            .as_ref()
            .map(|p| (self.game.playing_name(), p.loops()));
        if showing != self.watching {
            if let Some((name, loops)) = &showing {
                log(&format!(
                    "film {name} {}",
                    if *loops { "loops" } else { "plays once" }
                ));
            }
            self.watching = showing;
        }

        // A playing film supplies its own redraws; a static room only needs
        // one after a click.
        let mut soundtrack = None;
        if let Some(player) = &mut self.game.player {
            if player.tick() {
                self.dirty = true;
            }
            if !self.soundtrack_started {
                soundtrack = Some((
                    player.audio_for_segment(),
                    player.audio_rate,
                    player.audio_channels,
                ));
            }
        }
        if let Some((pcm, rate, channels)) = soundtrack {
            // Only counted as handed over once there is a mixer to hand it to.
            // The audio context needs a gesture and the opening film starts
            // before there has been one, so marking it done regardless left
            // the opening silent for the whole two minutes.
            if let Some(audio) = &self.audio {
                self.soundtrack_started = true;
                if rate > 0 && !pcm.is_empty() {
                    // Keyed, so leaving the room can stop it. QuickTime plays
                    // a movie's soundtrack outside the four channels, so it
                    // takes none of them either way.
                    audio.play(
                        None,
                        Some(FILM.to_string()),
                        pcm,
                        rate,
                        channels,
                        1.0,
                        false,
                        false,
                    );
                }
            }
        }
        if self.game.player.is_none() {
            self.soundtrack_started = false;
        }

        // Act on the release edge, so a click cannot fire twice, and not while
        // a sequence is running -- those open with `cursorOff` and the player
        // is meant to watch them.
        if !self.game.effects_busy() && self.was_down && !self.down {
            if let Some((x, y)) = self.pointer {
                if self.game.click_inventory(x, y, STAGE_W as i32, STAGE_H as i32) {
                    self.dirty = true;
                } else if let Some(outcome) = self.game.click(x, y) {
                    let _ = outcome;
                    self.dirty = true;
                }
            }
        }
        self.was_down = self.down;

        if self.dirty {
            self.game.inventory_hot = self.hot;
            self.game.draw(&mut self.frame, STAGE_W as u32, STAGE_H as u32);
            self.dirty = false;
        }
        if self.undither {
            // On the composed scene rather than on each plate, so the films
            // and the sprites over them are cleaned too.
            self.out
                .copy_from_slice(&amber::scale::undither(&self.frame, STAGE_W, STAGE_H));
        } else {
            self.out.copy_from_slice(&self.frame);
        }
        // A sequence takes the pointer away until it is done: `cursorOff` at
        // the top of a set piece, and the queue running dry is the `cursorOn`
        // this engine does not otherwise get. Without it the first set piece
        // took the pointer and never gave it back.
        if self.game.cursor_hidden && !self.game.effects_busy() && self.game.script_idle() {
            self.game.cursor_hidden = false;
        }
        if let Some((x, y)) = self.pointer.filter(|_| !self.game.cursor_hidden) {
            // The game's own cursor art, chosen by whatever verb is under the
            // pointer -- the same call the desktop makes.
            let verb = self.game.hotspot_at(x, y).map(|(v, _)| v);
            // The game's own art first; the drawn shapes are what is left when
            // a cursor is a system one -- `#back` and `#noCursor` have no cast
            // behind them -- so the player is never without a pointer. The
            // desktop has always done both and this did only the first, which
            // is why there was no pointer at all in most rooms.
            if !self
                .game
                .draw_cursor(&mut self.out, STAGE_W as u32, STAGE_H as u32, verb, x, y)
            {
                amber::cursor::draw(
                    &mut self.out,
                    STAGE_W as i32,
                    STAGE_H as i32,
                    x,
                    y,
                    verb,
                );
            }
        }
        for (i, px) in self.out.iter().enumerate() {
            let [b, g, r, _] = px.to_le_bytes();
            self.rgba[i * 4] = r;
            self.rgba[i * 4 + 1] = g;
            self.rgba[i * 4 + 2] = b;
            self.rgba[i * 4 + 3] = 255;
        }
        let image = web_sys::ImageData::new_with_u8_clamped_array_and_sh(
            Clamped(&self.rgba),
            STAGE_W as u32,
            STAGE_H as u32,
        )?;
        ctx.put_image_data(&image, 0.0, 0.0)?;

        let title = self.title();
        if title != self.title {
            self.title = title;
        }
        Ok(())
    }

    /// Plays what an effect asks for. The mixer is the only part of this the
    /// page can hear; everything visual has already been applied by
    /// `apply_puppet` inside the engine.
    fn apply(&mut self, effect: amber::script::Effect) {
        use amber::script::Effect;
        if self.game.apply_puppet(&effect) {
            self.dirty = true;
            return;
        }
        // The film effects are the front end's, not `apply_puppet`'s, and this
        // front end did not have them at all: `pushVideo` played nothing,
        // `killVideo` stopped nothing, and a scrubbed segment moved its state
        // and showed no picture. So every set piece was missing, the room's
        // own film was never taken down -- which is why the lake ghost ran on
        // after its sound had finished -- and its soundtrack outlived the room
        // it belonged to.
        match effect {
            Effect::PlayVideo(ref which) => {
                let which = which.clone();
                self.game.play_movie(which.as_deref());
                self.soundtrack_started = false;
                self.dirty = true;
                return;
            }
            Effect::StopVideo => {
                self.game.player = None;
                self.soundtrack_started = false;
                // The soundtrack is a one-shot in the mixer and knows nothing
                // about its picture being taken down.
                if let Some(audio) = &self.audio {
                    audio.stop(FILM);
                }
                self.dirty = true;
                return;
            }
            // A zero-length segment parks on a frame rather than playing,
            // which is what entering a room with the vane already turned
            // needs.
            Effect::PlayVideoSegment { from, to } => {
                if self.game.player.is_none() {
                    self.game.start_room_video();
                }
                if let Some(player) = &mut self.game.player {
                    player.play_segment(from, to);
                }
                self.dirty = true;
                return;
            }
            _ => {}
        }
        let Some(audio) = &self.audio else { return };
        match effect {
            Effect::PlaySound { name, loudness } => {
                let scale = match loudness.as_deref() {
                    Some("low") => 90.0 / 255.0,
                    Some("medium") => 180.0 / 255.0,
                    _ => 1.0,
                };
                let gain = self.game.sounds.gain(&name) * scale;
                if let Some((pcm, rate, ch)) = self.game.sound(&name) {
                    audio.play(Some(&name), None, pcm, rate, ch, gain, false, true);
                }
            }
            Effect::StartLoop { name, volume } => {
                let level = volume.unwrap_or(255) as f32 / 255.0;
                let gain = level * self.game.sounds.gain(&name);
                if let Some((pcm, rate, ch)) = self.game.sound(&name) {
                    audio.play(Some(&name), Some(name.clone()), pcm, rate, ch, gain, true, true);
                }
            }
            Effect::StopLoop { name, .. } => {
                audio.stop(&name);
                self.game.stop_program(&name);
            }
            // Fades are not modelled yet; the duck itself is what the scripts
            // rely on.
            Effect::SuspendSounds { .. } => audio.set_suspended(true),
            Effect::RestoreSounds { .. } => audio.set_suspended(false),
            Effect::StopGhostCall => {
                if let Some(n) = self.game.state.get("gLastCall").as_str() {
                    audio.stop_oneshot(n);
                }
            }
            _ => {}
        }
    }
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}
