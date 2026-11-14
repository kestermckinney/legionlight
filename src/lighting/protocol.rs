//! Lenovo 83Q7 OneZone static and firmware colour-pulse profiles.
//! Static matches the visually verified diagnostic; new effects follow the Windows source.
use crate::model::{Effect, LightingState, ProfileSet, Rgb, BRIGHTNESS_MAX, BRIGHTNESS_OFF};
use std::time::Duration;

pub const REPORT_ID: u8 = 0x07;
pub const REPORT_LEN: usize = 960;
pub const PROFILE_ID: u8 = 2;
pub const CMD_SELECT_PROFILE: u8 = 0xC8;
pub const CMD_SAVE_ZONES: u8 = 0xCB;
pub const CMD_BRIGHTNESS: u8 = 0xCE;
pub const CMD_QUERY_SELECTED: u8 = 0xCA;
pub const CMD_QUERY_PROFILE: u8 = 0xCC;
pub const CMD_QUERY_BRIGHTNESS: u8 = 0xCD;
pub const REPORT_SPACING: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EncodeError {
    #[error("brightness {value} is outside {BRIGHTNESS_OFF}..={BRIGHTNESS_MAX}")]
    Brightness { value: u8 },
    #[error("effect speed {value} is outside 1..=3")]
    Speed { value: u8 },
    #[error("invalid lighting settings: {0}")]
    Settings(String),
}

/// AA1 → CB static colour / LED 1 → C8 profile 2 → CE brightness → D0 DC/2.
/// GUI slots encode native Off; CLI Off without a slot only dims the saved colour.
pub fn reports_for(state: &LightingState) -> Result<Vec<[u8; REPORT_LEN]>, EncodeError> {
    if state.palette.len() > 4 || (state.effect == Effect::CustomCycle && state.palette.len() < 2) || state.profile.is_some_and(|p| !(1..=3).contains(&p)) {
        return Err(EncodeError::Settings("profile or palette is invalid".into()));
    }
    if !(1..=3).contains(&state.effect_speed) {
        return Err(EncodeError::Speed { value: state.effect_speed });
    }
    if state.brightness > BRIGHTNESS_MAX {
        return Err(EncodeError::Brightness { value: state.brightness });
    }
    if !state.enabled && state.profile.is_none() {
        return Ok(vec![packet(CMD_BRIGHTNESS, &[0])]);
    }
    Ok(vec![
        packet(0xAA, &[1, 0]),
        effect_profile(state.profile.unwrap_or_else(|| profile_id(state.effect)), state.enabled.then_some(state.effect), state.zones[0], state.effect_speed, &state.palette),
        packet(CMD_SELECT_PROFILE, &[state.profile.unwrap_or_else(|| profile_id(state.effect))]),
        packet(CMD_BRIGHTNESS, &[state.brightness]),
        packet(0xD0, &[0xDC, state.profile.unwrap_or_else(|| profile_id(state.effect))]),
    ])
}

/// Explicit destructive command; normal off never clears the profile.
pub fn clear_reports() -> Vec<[u8; REPORT_LEN]> {
    vec![packet(CMD_SELECT_PROFILE, &[PROFILE_ID]), packet(CMD_SAVE_ZONES, &[PROFILE_ID, 1, 1])]
}

fn profile_id(effect: Effect) -> u8 {
    if effect == Effect::Static { 2 } else { 1 }
}

/// Configure Lenovo's three Fn+Space slots, keeping the user's static colour.
/// Like Lenovo's default JSON: pulse/palette, static, off; then select static.
pub fn cycle_reports(state: &LightingState) -> Result<Vec<[u8; REPORT_LEN]>, EncodeError> {
    if !(1..=3).contains(&state.effect_speed) {
        return Err(EncodeError::Speed { value: state.effect_speed });
    }
    if state.brightness > BRIGHTNESS_MAX {
        return Err(EncodeError::Brightness { value: state.brightness });
    }
    Ok(vec![
        packet(0xAA, &[1, 0]),
        effect_profile(1, Some(Effect::ColourCycle), state.zones[0], state.effect_speed, &state.palette),
        effect_profile(2, Some(Effect::Static), state.zones[0], state.effect_speed, &state.palette),
        effect_profile(3, None, state.zones[0], state.effect_speed, &state.palette),
        packet(CMD_SELECT_PROFILE, &[2]),
        packet(CMD_BRIGHTNESS, &[state.brightness]),
        packet(0xD0, &[0xDC, 2]),
    ])
}

/// Save all three independently edited hotkey profiles, then activate the selected one.
pub fn profile_reports(set: &ProfileSet) -> Result<Vec<[u8; REPORT_LEN]>, EncodeError> {
    set.validate().map_err(EncodeError::Settings)?;
    let active = &set.profiles[set.selected];
    let mut reports = vec![packet(0xAA, &[1,0])];
    for (index, state) in set.profiles.iter().enumerate() {
        reports.push(effect_profile(index as u8 + 1,
            state.enabled.then_some(state.effect), state.zones[0], state.effect_speed, &state.palette));
    }
    let id = set.selected as u8 + 1;
    reports.push(packet(CMD_SELECT_PROFILE, &[id]));
    reports.push(packet(CMD_BRIGHTNESS, &[active.brightness]));
    reports.push(packet(0xD0, &[0xDC,id]));
    Ok(reports)
}

/// Query request: CC takes no payload; CA and CD take one zero byte (as Lenovo's tools send).
/// CC always answers for the *selected* slot, so reading all three means selecting each.
pub fn query_report(command: u8) -> [u8; REPORT_LEN] {
    packet(command, if command == CMD_QUERY_PROFILE { &[] } else { &[0] })
}

/// Select a slot (restores the selection after a read, mirroring the write sequence).
pub fn select_reports(slot: u8) -> Vec<[u8; REPORT_LEN]> {
    vec![packet(CMD_SELECT_PROFILE, &[slot]), packet(0xD0, &[0xDC, slot])]
}

/// Set the backlight level alone (`CE`). Level 0 blanks the panel without touching
/// the stored profiles, so restoring the previous level brings the same lighting back.
pub fn brightness_report(level: u8) -> Result<[u8; REPORT_LEN], EncodeError> {
    if level > BRIGHTNESS_MAX {
        return Err(EncodeError::Brightness { value: level });
    }
    Ok(packet(CMD_BRIGHTNESS, &[level]))
}

/// What the firmware stores for one slot. The custom palette is never stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FirmwareSlot {
    pub enabled: bool,
    pub effect: Effect,
    pub speed: u8,
    pub colour: Option<Rgb>,
}

/// Everything the firmware holds that the UI displays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirmwareProfiles {
    pub selected: usize,
    pub brightness: u8,
    pub slots: [FirmwareSlot; 3],
}

/// Value byte of a one-byte reply (`07 cmd 01 00 value`), as returned by CA and CD.
pub fn parse_value(command: u8, data: &[u8]) -> Result<u8, String> {
    if data.len() < 5 || data[0] != REPORT_ID || data[1] != command {
        return Err(format!("unexpected reply to query {command:#04X}"));
    }
    Ok(data[4])
}

/// Decode a CC reply — the same layout `effect_profile` writes. Returns (slot 1–3, settings).
pub fn parse_profile(data: &[u8]) -> Result<(u8, FirmwareSlot), String> {
    if data.len() < 4 || data[0] != REPORT_ID || data[1] != CMD_QUERY_PROFILE {
        return Err("unexpected reply to profile query".into());
    }
    let length = u16::from_le_bytes([data[2], data[3]]) as usize;
    let p = data.get(4..4 + length).filter(|p| p.len() >= 18).ok_or("profile reply is truncated")?;
    let key = |i: usize, want: u8| if p[i] == want { Ok(p[i + 1]) } else { Err(format!("profile reply has unexpected field layout at byte {i}")) };
    let (slot, animation, speed, colour_type) = (p[0], key(5, 1)?, key(7, 2)?, key(13, 5)?);
    if !(1..=3).contains(&slot) { return Err(format!("profile reply names slot {slot}")); }
    let colour = (p[17] == 1 && p.len() >= 21).then(|| Rgb::new(p[18], p[19], p[20]));
    let (enabled, effect) = match (animation, colour_type) {
        (0xFF, _) => (false, Effect::Static),
        (11, _) => (true, Effect::Static),
        (4, 1) => (true, Effect::ColourCycle),
        (4, _) => (true, Effect::Breathe),
        _ => return Err(format!("profile {slot} uses unsupported animation {animation}")),
    };
    Ok((slot, FirmwareSlot { enabled, effect, speed, colour }))
}

fn effect_profile(profile: u8, effect: Option<Effect>, c: Rgb, effect_speed: u8, palette: &[Rgb]) -> [u8; REPORT_LEN] {
    // OneZone a6220 calls animation vtable slot 0x30 for the wire effect.
    // AnimationColorPulse24Zone (JSON 21) be410 returns 4, not 21.
    // Colour type 1 selects firmware colours; 2 carries fixed RGB.
    let (animation, speed, colour_type, fixed) = match effect {
        Some(Effect::Static) => (11, 0, 2, true),
        Some(Effect::Breathe) => (4, effect_speed, 2, true),
        Some(Effect::ColourCycle) => (4, effect_speed, 1, false),
        // C615 native playback only uses the first custom colour. Save a steady
        // fallback; the worker plays the full palette through LampArray.
        Some(Effect::CustomCycle) => (11, 0, 2, true),
        // Random blend likewise stores its starting colour as the fallback.
        Some(Effect::RandomBlend) => (11, 0, 2, true),
        None => (0xFF, 0, 0, false),
    };
    let mut payload = vec![profile, 1, 1, 1, 6,
        1, animation, 2, speed, 3, 0, 4, 4, 5, colour_type, 6, 0];
    if fixed {
        let c = if effect == Some(Effect::CustomCycle) { palette[0] } else { c };
        payload.extend_from_slice(&[1, c.r, c.g, c.b]);
    } else {
        payload.push(0);
    }
    // Exactly one key, LED 1 in little-endian form.
    payload.extend_from_slice(&[1, 1, 0]);
    packet(CMD_SAVE_ZONES, &payload)
}

fn packet(command: u8, payload: &[u8]) -> [u8; REPORT_LEN] {
    let mut packet = [0; REPORT_LEN];
    packet[0] = REPORT_ID;
    packet[1] = command;
    packet[2..4].copy_from_slice(&(payload.len() as u16).to_le_bytes());
    packet[4..4 + payload.len()].copy_from_slice(payload);
    packet
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_visually_verified_python_sequence() {
        let reports = reports_for(&LightingState::uniform(Rgb::new(255, 0, 0)).brightness(9)).unwrap();
        let expected: &[&[u8]] = &[
            &[7, 0xAA, 2, 0, 1, 0],
            &[7, 0xCB, 24, 0, 2, 1, 1, 1, 6, 1, 11, 2, 0, 3, 0, 4, 4, 5, 2, 6, 0, 1, 255, 0, 0, 1, 1, 0],
            &[7, 0xC8, 1, 0, 2],
            &[7, 0xCE, 1, 0, 9],
            &[7, 0xD0, 2, 0, 0xDC, 2],
        ];
        assert_eq!(reports.len(), expected.len());
        for (actual, expected) in reports.iter().zip(expected) {
            assert_eq!(&actual[..expected.len()], *expected);
            assert!(actual[expected.len()..].iter().all(|b| *b == 0));
        }
    }
    #[test]
    fn arbitrary_colour_and_brightness_are_preserved() {
        for brightness in 1..=9 {
            let reports = reports_for(&LightingState::uniform(Rgb::new(17, 83, 201)).brightness(brightness)).unwrap();
            assert_eq!(&reports[1][22..25], &[17, 83, 201]);
            assert_eq!(reports[3][4], brightness);
        }
        assert!(reports_for(&LightingState::default().brightness(10)).is_err());
    }
    #[test]
    fn off_never_replaces_the_saved_colour() {
        let state = LightingState { enabled: false, ..LightingState::uniform(Rgb::WHITE) };
        let reports = reports_for(&state).unwrap();
        assert_eq!(reports, vec![packet(0xCE, &[0])]);
    }
    #[test]
    fn pulse_uses_recovered_wire_id_and_fixed_or_firmware_colours() {
        let mut state = LightingState::uniform(Rgb::new(17, 83, 201));
        state.effect = Effect::Breathe;
        let fixed = reports_for(&state).unwrap();
        assert_eq!(&fixed[1][..28], &[7,0xCB,24,0,1,1,1,1,6,1,4,2,2,3,0,4,4,5,2,6,0,1,17,83,201,1,1,0]);
        assert_eq!(fixed[2][4], 1);
        assert_eq!(&fixed[4][4..6], &[0xDC,1]);
        state.effect = Effect::ColourCycle;
        let auto = reports_for(&state).unwrap();
        assert_eq!(&auto[1][..25], &[7,0xCB,21,0,1,1,1,1,6,1,4,2,2,3,0,4,4,5,1,6,0,0,1,1,0]);
        assert!(auto[1][25..].iter().all(|b| *b == 0));
    }
    #[test]
    fn hotkey_cycle_saves_three_slots_and_selects_static() {
        let state = LightingState::uniform(Rgb::new(88,5,255)).brightness(9);
        let reports = cycle_reports(&state).unwrap();
        assert_eq!(reports.len(), 7);
        assert_eq!(&reports[1][4..5], &[1]);
        assert_eq!(&reports[2][22..25], &[88,5,255]);
        assert_eq!(&reports[3][..25], &[7,0xCB,21,0,3,1,1,1,6,1,0xFF,2,0,3,0,4,4,5,0,6,0,0,1,1,0]);
        assert_eq!(&reports[4][..5], &[7,0xC8,1,0,2]);
        assert_eq!(&reports[6][..6], &[7,0xD0,2,0,0xDC,2]);
        assert!(cycle_reports(&state.brightness(10)).is_err());
    }

    #[test]
    fn animation_speed_changes_only_supported_effect_properties() {
        let mut state = LightingState::uniform(Rgb::WHITE);
        for speed in 1..=3 {
            state.effect_speed = speed;
            state.effect = Effect::Breathe;
            assert_eq!(reports_for(&state).unwrap()[1][12], speed);
            state.effect = Effect::ColourCycle;
            assert_eq!(reports_for(&state).unwrap()[1][12], speed);
            assert_eq!(cycle_reports(&state).unwrap()[1][12], speed);
            state.effect = Effect::Static;
            assert_eq!(reports_for(&state).unwrap()[1][12], 0);
        }
        state.effect_speed = 0;
        assert!(reports_for(&state).is_err());
        assert!(state.validate().is_err());
        state.effect_speed = 4;
        assert!(cycle_reports(&state).is_err());
    }

    #[test]
    fn custom_palette_saves_a_steady_first_colour_fallback() {
        for count in 2..=4 {
            let mut state = LightingState::uniform(Rgb::WHITE);
            state.effect = Effect::CustomCycle;
            state.palette = (0..count).map(|i|Rgb::new(10+i,20+i,30+i)).collect();
            state.profile = Some(3);
            let reports = reports_for(&state).unwrap();
            let cb = &reports[1];
            assert_eq!(cb[4],3);
            assert_eq!(cb[10],11);
            assert_eq!(cb[12],0);
            assert_eq!(cb[21],1);
            assert_eq!(&cb[22..28],&[10,20,30,1,1,0]);
            assert_eq!(u16::from_le_bytes([cb[2],cb[3]]),24);
            assert!(cb[28..].iter().all(|b|*b==0));
            assert_eq!(reports[2][4],3);
            assert_eq!(reports[4][5],3);
        }
        let mut invalid = LightingState::default();
        invalid.effect=Effect::CustomCycle;
        invalid.palette=vec![Rgb::WHITE;5];
        assert!(reports_for(&invalid).is_err());
        invalid.palette=vec![Rgb::WHITE];
        assert!(reports_for(&invalid).is_err());
    }
    #[test]
    fn independently_edited_slots_preserve_effects_and_selection() {
        let mut set=ProfileSet::default();
        set.selected=2;
        set.profiles[0].effect=Effect::Static;
        set.profiles[0].zones[0]=Rgb::new(10,20,30);
        set.profiles[1].effect=Effect::CustomCycle;
        set.profiles[1].palette=vec![Rgb::new(255,0,0),Rgb::new(0,0,255)];
        set.profiles[2].enabled=true;
        set.profiles[2].effect=Effect::Breathe;
        set.profiles[2].effect_speed=1;
        let reports=profile_reports(&set).unwrap();
        assert_eq!(reports[1][10],11);
        assert_eq!(&reports[1][22..25],&[10,20,30]);
        assert_eq!(reports[2][21],1);
        assert_eq!(reports[3][12],1);
        assert_eq!(reports[4][4],3);
        assert_eq!(reports[6][5],3);
        set.profiles[1].profile=Some(3);
        assert!(profile_reports(&set).is_err());
    }
    #[test]
    fn off_slot_uses_supported_off_animation_without_zeroing_global_brightness() {
        let set=ProfileSet::default();
        let reports=reports_for(&set.profiles[2]).unwrap();
        assert_eq!(reports[1][10],0xff);
        assert_eq!(reports[1][4],3);
        assert_eq!(reports[3][4],7);
    }

    #[test]
    fn firmware_profiles_decode_what_the_encoder_writes() {
        let mut state = LightingState::uniform(Rgb::new(17, 83, 201));
        for (slot, effect, speed) in [(1u8, Effect::ColourCycle, 3u8), (2, Effect::Static, 2), (3, Effect::Breathe, 1)] {
            state.effect = effect; state.effect_speed = speed; state.profile = Some(slot);
            let mut reply = reports_for(&state).unwrap()[1]; reply[1] = CMD_QUERY_PROFILE;
            let (got, fw) = parse_profile(&reply).unwrap();
            assert_eq!((got, fw.enabled, fw.effect), (slot, true, effect));
            if effect != Effect::Static { assert_eq!(fw.speed, speed); }
            assert_eq!(fw.colour, (effect != Effect::ColourCycle).then(|| Rgb::new(17, 83, 201)));
        }
        let off = ProfileSet::default().profiles[2].clone();
        let mut reply = reports_for(&off).unwrap()[1]; reply[1] = CMD_QUERY_PROFILE;
        let (slot, fw) = parse_profile(&reply).unwrap();
        assert_eq!((slot, fw.enabled), (3, false));
        assert!(parse_profile(&[7, 0xCC, 0, 0]).is_err());
        assert_eq!(parse_value(0xCA, &[7, 0xCA, 1, 0, 2]).unwrap(), 2);
        assert!(parse_value(0xCD, &[7, 0xCA, 1, 0, 2]).is_err());
    }

}
