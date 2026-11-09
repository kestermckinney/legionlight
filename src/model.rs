//! Lighting state: the data the encoder serialises and the UI edits.
//!
//! No `gpui`, no `libc`, no file I/O. That keeps it unit-testable everywhere.

use serde::{Deserialize, Serialize};

/// This laptop's firmware-confirmed single RGB keyboard zone.
pub const ZONE_COUNT: usize = 1;

/// Raw brightness range accepted by command `0xCE`.
pub const BRIGHTNESS_MIN: u8 = 1;
pub const BRIGHTNESS_MAX: u8 = 9;

/// `0xCE` value that blanks the backlight without touching the stored frame.
///
/// Observed on `048d:c615`: accepted, the panel goes dark, and the zone table
/// survives. That makes it the safe "off" — unlike a blank zone table, which
/// leaves the firmware hotkey nothing to replay.
pub const BRIGHTNESS_OFF: u8 = 0;

/// Lenovo's own shipped default (`NBLightingConfig/brightness.json`).
pub const BRIGHTNESS_DEFAULT: u8 = 7;

/// Schema version of the persisted document.
pub const STATE_VERSION: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const OFF: Rgb = Rgb { r: 0, g: 0, b: 0 };
    pub const WHITE: Rgb = Rgb { r: 255, g: 255, b: 255 };

    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Parse `R,G,B` with each component in `0..=255`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut parts = text.split(',');
        let mut next = |name: &str| -> Result<u8, String> {
            let raw = parts
                .next()
                .ok_or_else(|| format!("expected `R,G,B`, got `{text}`"))?
                .trim();
            raw.parse::<u8>()
                .map_err(|_| format!("{name} is not a byte in 0..=255: `{raw}`"))
        };
        let colour = Self::new(next("red")?, next("green")?, next("blue")?);
        if parts.next().is_some() {
            return Err(format!("expected `R,G,B`, got `{text}`"));
        }
        Ok(colour)
    }

    pub fn hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    pub fn is_off(self) -> bool {
        self == Self::OFF
    }
}

impl Default for Rgb {
    fn default() -> Self {
        Self::OFF
    }
}

impl Rgb {
    pub fn parse_hex(value: &str) -> Result<Self, String> {
        let value = value.trim().trim_start_matches('#');
        if value.len() != 6 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Enter six hex digits, such as FF8000".into());
        }
        let n = u32::from_str_radix(value, 16).map_err(|e| e.to_string())?;
        Ok(Self::new((n >> 16) as u8, (n >> 8) as u8, n as u8))
    }
}

/// OneZone exposes static animation 11 and colour-pulse animation 21.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    #[default]
    Static,
    Breathe,
    ColourCycle,
    CustomCycle,
}
impl Effect {
    pub fn label(self) -> &'static str {
        match self {
            Self::Static => "Static",
            Self::Breathe => "Breathe",
            Self::ColourCycle => "Lenovo colours",
            Self::CustomCycle => "Custom palette",
        }
    }
}

/// One complete keyboard lighting configuration.
///
/// `zones` is indexed from zero; the wire protocol addresses zones from one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightingState {
    pub version: u8,
    pub enabled: bool,
    pub brightness: u8,
    #[serde(default)]
    pub effect: Effect,
    #[serde(default = "default_effect_speed")]
    pub effect_speed: u8,
    #[serde(deserialize_with = "deserialize_zones")]
    pub zones: [Rgb; ZONE_COUNT],
    #[serde(default)]
    pub palette: Vec<Rgb>,
    #[serde(default)]
    pub profile: Option<u8>,
}

fn default_effect_speed() -> u8 { 2 }

// Preserve version-1 presets from the old 24-zone UI by using their first colour.
fn deserialize_zones<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<[Rgb; ZONE_COUNT], D::Error> {
    let colours = Vec::<Rgb>::deserialize(deserializer)?;
    match colours.len() {
        1 | 24 => Ok([colours[0]]),
        _ => Err(serde::de::Error::custom("expected one colour or a legacy 24-zone preset")),
    }
}

impl Default for LightingState {
    fn default() -> Self {
        Self {
            version: STATE_VERSION,
            enabled: true,
            brightness: BRIGHTNESS_DEFAULT,
            effect: Effect::Static,
            effect_speed: default_effect_speed(),
            zones: [Rgb::OFF; ZONE_COUNT],
            palette: Vec::new(),
            profile: None,
        }
    }
}

impl LightingState {
    /// Every zone the same colour.
    pub fn uniform(colour: Rgb) -> Self {
        Self { zones: [colour; ZONE_COUNT], ..Self::default() }
    }

    pub fn brightness(mut self, brightness: u8) -> Self {
        self.brightness = brightness;
        self
    }

    /// Toggle one zone using a **1-based** zone number, as printed by the firmware
    /// tooling and as used in `--zone N=R,G,B`.
    pub fn set_zone(&mut self, zone: u8, colour: Rgb) -> Result<(), String> {
        let index = zone as usize;
        if zone == 0 || index > ZONE_COUNT {
            return Err(format!("zone {zone} is outside 1..={ZONE_COUNT}"));
        }
        self.zones[index - 1] = colour;
        Ok(())
    }

    pub fn zone(&self, zone: u8) -> Option<Rgb> {
        let index = zone as usize;
        if zone == 0 || index > ZONE_COUNT {
            return None;
        }
        Some(self.zones[index - 1])
    }

    pub fn is_off(&self) -> bool {
        !self.enabled || self.zones.iter().all(|colour| colour.is_off())
    }

    /// Number of distinct colours, which is also the number of groups in the
    /// `0xCB` report.
    pub fn distinct_colours(&self) -> usize {
        let mut seen: Vec<Rgb> = Vec::with_capacity(ZONE_COUNT);
        for colour in &self.zones {
            if !seen.contains(colour) {
                seen.push(*colour);
            }
        }
        seen.len()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.profile.is_some_and(|p| !(1..=3).contains(&p)) {
            return Err("profile must be 1, 2, or 3".into());
        }
        if self.palette.len() > 4 || (self.effect == Effect::CustomCycle && self.palette.len() < 2) {
            return Err("custom palettes need 2–4 colours".into());
        }
        if !(1..=3).contains(&self.effect_speed) {
            return Err("effect speed is outside 1..=3".into());
        }
        if self.version != STATE_VERSION {
            return Err(format!("unsupported state version {} (expected {STATE_VERSION})", self.version));
        }
        if !(BRIGHTNESS_MIN..=BRIGHTNESS_MAX).contains(&self.brightness) {
            return Err(format!(
                "brightness {} is outside {BRIGHTNESS_MIN}..={BRIGHTNESS_MAX}",
                self.brightness
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_colour() {
        assert_eq!(Rgb::parse("1,2,3").unwrap(), Rgb::new(1, 2, 3));
        assert_eq!(Rgb::parse(" 255 , 0 , 16 ").unwrap(), Rgb::new(255, 0, 16));
        assert!(Rgb::parse("256,0,0").is_err());
        assert!(Rgb::parse("1,2").is_err());
        assert!(Rgb::parse("1,2,3,4").is_err());
        assert!(Rgb::parse("red,0,0").is_err());
    }

    #[test]
    fn zones_are_one_based_on_the_outside() {
        let mut state = LightingState::default();
        state.set_zone(1, Rgb::WHITE).unwrap();
        assert_eq!(state.zones[0], Rgb::WHITE);
        assert!(state.set_zone(2, Rgb::WHITE).is_err());
        assert!(state.set_zone(0, Rgb::WHITE).is_err());
        assert!(state.set_zone(25, Rgb::WHITE).is_err());
    }

    #[test]
    fn counts_distinct_colours() {
        let mut state = LightingState::default();
        assert_eq!(state.distinct_colours(), 1);
        state.zones[0] = Rgb::WHITE;
        assert_eq!(state.distinct_colours(), 1);
    }
}

#[cfg(test)]
mod preset_tests {
    use super::*;
    #[test]
    fn old_presets_use_first_colour_and_save_one_zone() {
        let json = serde_json::json!({"version":1,"enabled":true,"brightness":7,
            "zones": vec![Rgb::new(12,34,56);24]});
        let state: LightingState = serde_json::from_value(json).unwrap();
        assert_eq!(state.zones, [Rgb::new(12,34,56)]);
        state.validate().unwrap();
        let saved = serde_json::to_value(&state).unwrap();
        assert_eq!(saved["zones"].as_array().unwrap().len(), 1);
        assert_eq!(serde_json::from_value::<LightingState>(saved).unwrap(), state);
    }
    #[test]
    fn effects_round_trip_and_old_presets_default_to_static() {
        let old = serde_json::json!({"version":1,"enabled":true,"brightness":7,
            "zones":[{"r":255,"g":0,"b":0}]});
        let mut state: LightingState = serde_json::from_value(old).unwrap();
        assert_eq!(state.effect, Effect::Static);
        for effect in [Effect::Static, Effect::Breathe, Effect::ColourCycle] {
            state.effect = effect;
            let saved = serde_json::to_vec(&state).unwrap();
            assert_eq!(serde_json::from_slice::<LightingState>(&saved).unwrap(), state);
        }
    }

}

/// Local drafts for the three firmware hotkey slots. Loading never sends USB writes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileSet {
    pub selected: usize,
    pub profiles: [LightingState; 3],
}
impl Default for ProfileSet {
    fn default() -> Self {
        let mut animated = LightingState::uniform(Rgb::new(21,141,221));
        animated.profile = Some(1);
        animated.effect = Effect::ColourCycle;
        let mut steady = LightingState::uniform(Rgb::new(88,5,255));
        steady.profile = Some(2);
        let mut off = steady.clone();
        off.profile = Some(3);
        off.enabled = false;
        Self { selected: 1, profiles: [animated, steady, off] }
    }
}
impl ProfileSet {
    pub fn validate(&self) -> Result<(), String> {
        if self.selected > 2 { return Err("selected profile must be 1–3".into()); }
        for (index, state) in self.profiles.iter().enumerate() {
            state.validate()?;
            if state.profile != Some(index as u8 + 1) {
                return Err("profile slots must be in order 1–3".into());
            }
        }
        Ok(())
    }
    pub fn from_json(bytes: &[u8]) -> Result<Self, String> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Saved { Profiles(ProfileSet), Legacy(LightingState) }
        let set = match serde_json::from_slice::<Saved>(bytes).map_err(|e| e.to_string())? {
            Saved::Profiles(set) => set,
            Saved::Legacy(mut state) => {
                state.validate()?;
                state.profile = Some(2);
                let mut set = Self::default();
                set.profiles[1] = state;
                set
            }
        };
        set.validate()?;
        Ok(set)
    }
}

#[cfg(test)]
mod profile_set_tests {
    use super::*;
    #[test]
    fn hex_rgb_and_palette_validation() {
        assert_eq!(Rgb::parse_hex("#fF8000").unwrap(),Rgb::new(255,128,0));
        assert!(Rgb::parse_hex("ffffff00").is_err());
        assert!(Rgb::parse_hex("nope").is_err());
        assert!(Rgb::parse_hex("é00000").is_err());
        let mut state=LightingState::default();
        state.effect=Effect::CustomCycle;
        assert!(state.validate().is_err());
        state.palette=vec![Rgb::WHITE;4];
        state.validate().unwrap();
        state.palette.push(Rgb::OFF);
        assert!(state.validate().is_err());
    }
    #[test]
    fn full_profile_set_round_trips_and_legacy_presets_survive() {
        let mut set=ProfileSet::default();
        set.profiles[0].effect=Effect::CustomCycle;
        set.profiles[0].palette=vec![Rgb::new(12,34,56),Rgb::new(78,90,123)];
        set.selected=0;
        assert_eq!(ProfileSet::from_json(&serde_json::to_vec(&set).unwrap()).unwrap(),set);
        let old=LightingState::uniform(Rgb::new(1,2,3));
        let migrated=ProfileSet::from_json(&serde_json::to_vec(&old).unwrap()).unwrap();
        assert_eq!(migrated.profiles[1].zones[0],Rgb::new(1,2,3));
        assert_eq!(migrated.profiles[1].profile,Some(2));
        set.selected=3;
        assert!(ProfileSet::from_json(&serde_json::to_vec(&set).unwrap()).is_err());
    }
}
