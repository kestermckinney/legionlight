use gpui::*;
use legion_light::{lighting::worker::{Command, Worker, SHUTDOWN_COMPLETE}, model::{Effect, LightingState, ProfileSet, Rgb}};
use std::{path::PathBuf, time::Duration};

pub struct LightingApp {
    pub state: LightingState,
    pub profiles: ProfileSet,
    pub palette_index: usize,
    pub colour_text: String,
    pub editing: bool,
    pub select_all: bool,
    pub cursor: usize,
    pub focus: FocusHandle,
    pub font: Option<SharedString>,
    pub quit_requested: bool,
    pub status: String,
    pub about: bool,
    pub worker: Worker,
}
impl LightingApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(100)).await;
            if this.update(cx, |app, cx| {
                app.poll_worker(cx);
            }).is_err() { break; }
        }).detach();
        let (profiles, startup_status) = match Self::preset_path() {
            Ok(path) => match std::fs::read(path) {
                Ok(bytes) => match ProfileSet::from_json(&bytes) {
                    Ok(set) => (set, "Saved drafts loaded — nothing applied yet".to_string()),
                    Err(error) => (ProfileSet::default(), format!("Saved drafts could not load: {error}")),
                },
                Err(error) if error.kind() == std::io::ErrorKind::NotFound =>
                    (ProfileSet::default(), "Ready — choose a colour or profile to apply".to_string()),
                Err(error) => (ProfileSet::default(), format!("Saved drafts could not load: {error}")),
            },
            Err(error) => (ProfileSet::default(), error),
        };
        let state = profiles.profiles[profiles.selected].clone();
        let focus = cx.focus_handle();
        window.focus(&focus);
        let font = cx.text_system().all_font_names().into_iter().find(|n| n == "Inter").map(SharedString::from);
        let initial_colour = if state.effect == Effect::CustomCycle { state.palette[0] } else { state.zones[0] };
        Self { colour_text: initial_colour.hex(), cursor: 7, state, profiles,
            palette_index: 0, editing: false, select_all: false, focus, font,
            quit_requested: false, status: startup_status, about: false, worker: Worker::start() }
    }
    pub fn poll_worker(&mut self, cx: &mut Context<Self>) {
        while let Ok(status) = self.worker.events.try_recv() {
            if status == SHUTDOWN_COMPLETE && self.quit_requested {
                self.status = "Keyboard playback stopped".into();
                cx.quit();
            } else if !self.quit_requested {
                self.status = status;
                cx.notify();
            }
        }
    }
    pub fn quit(&mut self, cx: &mut Context<Self>) {
        if self.quit_requested { return; }
        self.quit_requested = true;
        self.status = "Returning control to the keyboard…".into();
        if self.worker.commands.send(Command::Shutdown).is_err() { cx.quit(); }
        cx.notify();
    }
    fn store_draft(&mut self) {
        self.state.profile = Some(self.profiles.selected as u8 + 1);
        self.profiles.profiles[self.profiles.selected] = self.state.clone();
    }
    pub fn apply(&mut self, cx: &mut Context<Self>) {
        if self.quit_requested { return; }
        self.store_draft();
        if let Err(error) = self.state.validate() { self.status = error; cx.notify(); return; }
        self.status = match self.worker.commands.send(Command::Apply(self.state.clone())) {
            Ok(()) => "Applying profile…".into(), Err(e) => e.to_string()
        };
        cx.notify();
    }
    pub fn preview_colour(&self) -> Rgb {
        if self.state.effect == Effect::CustomCycle {
            self.state.palette.get(self.palette_index).copied().unwrap_or(self.state.zones[0])
        } else { self.state.zones[0] }
    }
    pub fn sync_text(&mut self) {
        self.colour_text = self.preview_colour().hex();
        self.cursor = self.colour_text.len();
        self.select_all = false;
    }
    pub fn colour(&mut self, colour: Rgb, cx: &mut Context<Self>) {
        if self.state.effect == Effect::CustomCycle {
            if let Some(entry) = self.state.palette.get_mut(self.palette_index) { *entry = colour; }
        } else {
            self.state.zones[0] = colour;
            if self.state.effect == Effect::ColourCycle { self.state.effect = Effect::Static; }
        }
        self.state.enabled = true;
        self.sync_text();
        self.apply(cx);
    }
    pub fn effect(&mut self, effect: Effect, cx: &mut Context<Self>) {
        self.state.effect = effect;
        self.state.enabled = true;
        if effect == Effect::CustomCycle && self.state.palette.len() < 2 {
            self.state.palette = vec![self.state.zones[0], Rgb::new(0,80,255)];
        }
        self.palette_index = 0;
        self.sync_text();
        self.apply(cx);
    }
    pub fn select_profile(&mut self, index: usize, cx: &mut Context<Self>) {
        self.store_draft();
        self.profiles.selected = index;
        self.state = self.profiles.profiles[index].clone();
        self.palette_index = 0;
        self.editing = false;
        self.sync_text();
        self.status = format!("Editing profile {} — Apply to activate it", index + 1);
        cx.notify();
    }
    pub fn save_profiles(&mut self, cx: &mut Context<Self>) {
        if self.quit_requested { return; }
        self.store_draft();
        if let Err(error) = self.profiles.validate() { self.status = error; cx.notify(); return; }
        self.status = match self.worker.commands.send(Command::ConfigureProfiles(self.profiles.clone())) {
            Ok(()) => "Saving Fn+Space profiles…".into(), Err(error) => error.to_string()
        };
        cx.notify();
    }
    pub fn defaults(&mut self, cx: &mut Context<Self>) {
        self.profiles = ProfileSet::default();
        self.state = self.profiles.profiles[1].clone();
        self.palette_index = 0;
        self.sync_text();
        self.save_profiles(cx);
    }
    pub fn palette_add(&mut self, cx: &mut Context<Self>) {
        if self.state.palette.len() < 4 {
            self.state.palette.push(Rgb::WHITE);
            self.palette_index = self.state.palette.len()-1;
            self.sync_text();
            self.apply(cx);
        }
    }
    pub fn palette_remove(&mut self, cx: &mut Context<Self>) {
        if self.state.palette.len() > 2 {
            self.state.palette.remove(self.palette_index);
            self.palette_index = self.palette_index.min(self.state.palette.len()-1);
            self.sync_text();
            self.apply(cx);
        }
    }
    pub fn custom_colour(&mut self, cx: &mut Context<Self>) {
        let parsed = if self.colour_text.contains(',') { Rgb::parse(&self.colour_text) }
            else { Rgb::parse_hex(&self.colour_text) };
        match parsed {
            Ok(colour) => self.colour(colour, cx),
            Err(error) => { self.status = error; cx.notify(); }
        }
    }
    pub fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        if (key.modifiers.control || key.modifiers.platform) && key.key == "q" { self.quit(cx); return; }
        if key.key == "tab" {
            self.editing = !self.editing;
            self.select_all = self.editing;
            cx.notify(); return;
        }
        if !self.editing { return; }
        if key.key == "escape" { self.editing = false; self.sync_text(); cx.notify(); return; }
        if key.key == "enter" { self.custom_colour(cx); return; }
        let modifier = key.modifiers.control || key.modifiers.platform;
        if modifier && key.key == "a" { self.select_all = true; cx.notify(); return; }
        if modifier && (key.key == "c" || key.key == "x") && self.select_all {
            cx.write_to_clipboard(ClipboardItem::new_string(self.colour_text.clone()));
            if key.key == "x" { self.colour_text.clear(); self.cursor = 0; self.select_all = false; }
            cx.notify(); return;
        }
        let inserted = if modifier && key.key == "v" { cx.read_from_clipboard().and_then(|item| item.text()) }
            else if !modifier && !key.modifiers.alt { key.key_char.clone() } else { None };
        if let Some(text) = inserted {
            let text: String = text.chars().filter(|c| c.is_ascii() && !c.is_control()).take(32).collect();
            if !text.is_empty() {
                if self.select_all { self.colour_text.clear(); self.cursor = 0; }
                self.select_all = false;
                let available = 32usize.saturating_sub(self.colour_text.len());
                let text = &text[..text.len().min(available)];
                self.colour_text.insert_str(self.cursor, text);
                self.cursor += text.len();
            }
        } else {
            match key.key.as_str() {
                "backspace" | "delete" if self.select_all => { self.colour_text.clear(); self.cursor = 0; self.select_all = false; }
                "backspace" if self.cursor > 0 => { self.cursor -= 1; self.colour_text.remove(self.cursor); }
                "delete" if self.cursor < self.colour_text.len() => { self.colour_text.remove(self.cursor); }
                "left" => { self.select_all = false; self.cursor = self.cursor.saturating_sub(1); }
                "right" => { self.select_all = false; self.cursor = (self.cursor+1).min(self.colour_text.len()); }
                "home" => { self.select_all = false; self.cursor = 0; }
                "end" => { self.select_all = false; self.cursor = self.colour_text.len(); }
                _ => {}
            }
        }
        cx.notify();
    }
    fn preset_path() -> Result<PathBuf, String> {
        let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")))
            .ok_or("Cannot locate configuration directory")?;
        Ok(base.join("legionlight/presets/saved.json"))
    }
    pub fn preset(&mut self, save: bool, cx: &mut Context<Self>) {
        self.store_draft();
        let result = (|| -> Result<(), String> {
            let path = Self::preset_path()?;
            if save {
                self.profiles.validate()?;
                std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
                let temporary = path.with_extension("json.tmp");
                std::fs::write(&temporary, serde_json::to_vec_pretty(&self.profiles).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
                std::fs::rename(temporary, path).map_err(|e|e.to_string())?;
                self.status = "All three profile drafts saved to disk".into();
            } else {
                self.profiles = ProfileSet::from_json(&std::fs::read(path).map_err(|e|e.to_string())?)?;
                self.state = self.profiles.profiles[self.profiles.selected].clone();
                self.palette_index = 0;
                self.sync_text();
                self.status = "Profile drafts loaded — Apply or Save Fn+Space to send".into();
            }
            Ok(())
        })();
        if let Err(error) = result { self.status = error; }
        cx.notify();
    }
}
