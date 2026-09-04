//! The desktop window, which is the only host with a platform behind it.

use minifb::{Key as MKey, MouseButton, MouseMode, Window, WindowOptions};

use crate::host::{Host, Input, Key};

pub struct Desktop {
    window: Window,
}

impl Desktop {
    pub fn open(title: &str, stage: (usize, usize)) -> Result<Desktop, minifb::Error> {
        let mut window = Window::new(
            title,
            stage.0,
            stage.1,
            WindowOptions {
                scale: minifb::Scale::X1,
                resize: true,
                scale_mode: minifb::ScaleMode::AspectRatioStretch,
                ..WindowOptions::default()
            },
        )?;
        // The original runs at a nominal 15 fps; this only caps the loop, and
        // the room is static between clicks anyway.
        window.set_target_fps(60);
        // The game draws its own pointer into the frame, so the desktop's
        // would be a second one sitting on top of it.
        window.set_cursor_visibility(false);
        Ok(Desktop { window })
    }
}

impl Host for Desktop {
    fn poll(&mut self, stage: (usize, usize)) -> Input {
        // The window may be resized, but the stage is a fixed size and the
        // scale mode letterboxes it, so pointer coordinates map back here --
        // this is the only place that knows how the scaling was done.
        let (win_w, win_h) = self.window.get_size();
        let scale = (win_w as f32 / stage.0 as f32).min(win_h as f32 / stage.1 as f32);
        let (ox, oy) = (
            (win_w as f32 - stage.0 as f32 * scale) / 2.0,
            (win_h as f32 - stage.1 as f32 * scale) / 2.0,
        );

        let pointer = self
            .window
            .get_mouse_pos(MouseMode::Pass)
            .map(|(x, y)| (((x - ox) / scale) as i32, ((y - oy) / scale) as i32));

        let mut pressed = Vec::new();
        for (theirs, ours) in [
            (MKey::Space, Key::Space),
            (MKey::Tab, Key::Hotspots),
            (MKey::S, Key::Stage),
            (MKey::C, Key::Cut),
        ] {
            if self.window.is_key_pressed(theirs, minifb::KeyRepeat::No) {
                pressed.push(ours);
            }
        }
        if self.window.is_key_down(MKey::Escape) {
            pressed.push(Key::Escape);
        }

        Input {
            pointer,
            down: self.window.get_mouse_down(MouseButton::Left),
            pressed,
            open: self.window.is_open(),
        }
    }

    fn present(&mut self, frame: &[u32], stage: (usize, usize)) -> std::io::Result<()> {
        self.window
            .update_with_buffer(frame, stage.0, stage.1)
            .map_err(std::io::Error::other)
    }

    fn set_title(&mut self, title: &str) {
        self.window.set_title(title);
    }
}
