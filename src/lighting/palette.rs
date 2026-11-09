//! App-driven palette playback using the experimentally verified standard LampArray path.
//! Frames never write CB profiles. The saved profile holds the first colour as fallback.
use std::{thread, time::{Duration, Instant}};
use super::{hid, protocol};
use crate::model::{Effect, LightingState};

pub fn palette_period(speed: u8) -> Duration {
    Duration::from_secs(match speed { 1 => 3, 3 => 1, _ => 2 })
}

/// Smoothly fade each entry up/down, changing hue at darkness to avoid a flash.
pub fn frame(state: &LightingState, elapsed: Duration) -> [u8; 10] {
    let period = palette_period(state.effect_speed).as_secs_f64();
    let position = elapsed.as_secs_f64() / period;
    let index = position.floor() as usize % state.palette.len();
    let colour = state.palette[index];
    let intensity = (std::f64::consts::PI * position.fract()).sin().powi(2)
        * f64::from(state.brightness) / 9.0;
    let scale = |channel: u8| (f64::from(channel)*intensity).round().clamp(0.,255.) as u8;
    [5,1,0,0,0,0,scale(colour.r),scale(colour.g),scale(colour.b),1]
}

pub struct Session {
    vendor: hid::Device,
    lamp: hid::Device,
    state: LightingState,
    started: Instant,
    last_poll: Instant,
    selected_at_start: u8,
    restore_profile: u8,
    restore_brightness: u8,
    armed: bool,
    pub interval: Duration,
}
impl Session {
    pub fn begin(state: LightingState) -> Result<Self,String> {
        state.validate()?;
        if !state.enabled || state.effect != Effect::CustomCycle {
            return Err("Live playback needs an enabled custom palette".into());
        }
        let vendor=hid::Device::open_for_write(hid::lighting_node().map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        vendor.ensure_lighting().map_err(|e|e.to_string())?;
        let lamp=hid::Device::open_for_write(hid::lamparray_node().map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        if !lamp.is_lamparray() { return Err("Expected keyboard LampArray interface".into()); }
        let attributes=lamp.get_feature(1,23).map_err(|e|e.to_string())?;
        if attributes.len()!=23 || attributes[0]!=1 || u16::from_le_bytes([attributes[1],attributes[2]])!=1 {
            return Err("Expected one programmable keyboard lamp".into());
        }
        let minimum=Duration::from_micros(u32::from_le_bytes(attributes[19..23].try_into().unwrap()) as u64);
        let mut session=Self { vendor,lamp, restore_profile:state.profile.unwrap_or(1),
            restore_brightness:state.brightness,state,started:Instant::now(),last_poll:Instant::now(),
            selected_at_start:0,armed:false,interval:minimum.max(Duration::from_millis(50)) };
        session.armed=true; // Any error from this point must release both control paths.
        session.vendor.send(&vendor_packet(0xAA,&[0,0])).map_err(|e|e.to_string())?;
        thread::sleep(Duration::from_millis(150));
        session.lamp.send(&[6,0]).map_err(|e|e.to_string())?;
        thread::sleep(Duration::from_millis(50));
        session.selected_at_start=session.query_profile()?;
        session.started=Instant::now();
        session.lamp.send(&frame(&session.state,Duration::ZERO)).map_err(|e|e.to_string())?;
        Ok(session)
    }
    fn query_profile(&self) -> Result<u8,String> {
        self.vendor.send(&vendor_packet(0xCA,&[0])).map_err(|e|e.to_string())?;
        thread::sleep(Duration::from_millis(50));
        let data=self.vendor.get_feature(7,960).map_err(|e|e.to_string())?;
        if data.len()<5 || data[..2]!=[7,0xCA] { return Err("Unexpected profile query reply".into()); }
        Ok(data[4])
    }
    /// Returns false when the firmware hotkey changes the selected native profile.
    pub fn tick(&mut self) -> Result<bool,String> {
        if self.last_poll.elapsed()>=Duration::from_millis(500) {
            let current=self.query_profile()?;
            self.last_poll=Instant::now();
            if current<=3 && current!=self.selected_at_start {
                if current==0 { self.restore_brightness=0; }
                else { self.restore_profile=current; }
                return Ok(false);
            }
        }
        self.lamp.send(&frame(&self.state,self.started.elapsed())).map_err(|e|e.to_string())?;
        Ok(true)
    }
    pub fn finish(&mut self) -> Result<(),String> {
        if !self.armed { return Ok(()); }
        let mut errors=Vec::new();
        // Attempt every restoration step even if a device disappears midway.
        if let Err(e)=self.lamp.send(&[6,1]) { errors.push(e.to_string()); }
        for packet in [vendor_packet(0xAA,&[1,0]),vendor_packet(0xC8,&[self.restore_profile]),
            vendor_packet(0xCE,&[self.restore_brightness]),vendor_packet(0xD0,&[0xDC,self.restore_profile])] {
            if let Err(e)=self.vendor.send(&packet) { errors.push(e.to_string()); }
            thread::sleep(protocol::REPORT_SPACING);
        }
        self.armed=false;
        if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if let Err(e)=self.finish() { eprintln!("Legion Light: restoring LampArray control failed: {e}"); }
    }
}
fn vendor_packet(command:u8,payload:&[u8])->[u8;960] {
    let mut out=[0;960];out[0]=7;out[1]=command;
    out[2..4].copy_from_slice(&(payload.len() as u16).to_le_bytes());
    out[4..4+payload.len()].copy_from_slice(payload);out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Rgb;
    fn state()->LightingState {
        let mut s=LightingState::uniform(Rgb::WHITE).brightness(9);
        s.effect=Effect::CustomCycle;
        s.palette=vec![Rgb::new(255,0,0),Rgb::new(0,255,0),Rgb::new(0,0,255),Rgb::WHITE];s
    }
    #[test]
    fn every_colour_is_sent_and_the_palette_wraps() {
        let s=state();
        for (i,c) in s.palette.iter().chain(s.palette.iter().take(1)).enumerate() {
            let f=frame(&s,Duration::from_secs(i as u64*2+1));
            assert_eq!(f,[5,1,0,0,0,0,c.r,c.g,c.b,1]);
        }
    }
    #[test]
    fn colour_changes_happen_in_darkness_and_brightness_scales_rgb() {
        let mut s=state();
        assert_eq!(&frame(&s,Duration::ZERO)[6..9],&[0,0,0]);
        assert_eq!(&frame(&s,Duration::from_secs(2))[6..9],&[0,0,0]);
        s.brightness=3;
        assert_eq!(&frame(&s,Duration::from_secs(1))[6..9],&[85,0,0]);
        assert_eq!(palette_period(1),Duration::from_secs(3));
        assert_eq!(palette_period(3),Duration::from_secs(1));
    }
}
