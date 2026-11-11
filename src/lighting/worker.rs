//! Serialized profile reads/writes and transient LampArray playback.
use std::{sync::mpsc::{self, Receiver, Sender}, thread, time::{Duration, Instant}};
use super::{hid, palette, protocol};
use crate::model::{Effect, LightingState, ProfileSet};
use super::protocol::FirmwareProfiles;

pub const SHUTDOWN_COMPLETE: &str = "__legion_shutdown_complete__";
pub enum Command {
    Apply(LightingState),
    ConfigureProfiles(ProfileSet),
    ReadProfiles,
    Shutdown,
}
pub struct Worker { pub commands: Sender<Command>, pub events: Receiver<String>, pub loaded: Receiver<FirmwareProfiles> }
impl Worker {
    pub fn start() -> Self {
        let (commands, rx) = mpsc::channel::<Command>();
        let (tx, events) = mpsc::channel();
        let (loaded_tx, loaded) = mpsc::channel();
        thread::spawn(move || {
            let mut device = None;
            let mut live: Option<palette::Session> = None;
            let mut pending = None;
            loop {
                let received = if let Some(command)=pending.take() { Ok(command) }
                    else if let Some(session)=live.as_ref() { rx.recv_timeout(session.interval) }
                    else { rx.recv().map_err(|_|mpsc::RecvTimeoutError::Disconnected) };
                let mut command = match received {
                    Ok(command) => command,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        let tick=live.as_mut().unwrap().tick();
                        match tick {
                            Ok(true) => {},
                            Ok(false) => {
                                drop(live.take());
                                let _=tx.send("Custom palette stopped — firmware hotkey selected a profile".into());
                            },
                            Err(error) => {
                                drop(live.take());
                                let _=tx.send(format!("Palette stopped: {error}"));
                            },
                        }
                        continue;
                    },
                };
                // Coalesce edits to one slot, preserving other slots and explicit setup.
                if matches!(command,Command::Apply(_)) {
                    loop {
                        match rx.recv_timeout(Duration::from_millis(150)) {
                            Ok(next @ Command::Apply(_)) => {
                                let same_slot=match (&command,&next) {
                                    (Command::Apply(a),Command::Apply(b))=>a.profile==b.profile,
                                    _=>false,
                                };
                                if same_slot { command=next; }
                                else { pending=Some(next);break; }
                            },
                            Ok(Command::Shutdown)=>{command=Command::Shutdown;break;},
                            Ok(next)=>{pending=Some(next);break;},
                            Err(_)=>break,
                        }
                    }
                }
                if matches!(command,Command::Shutdown) { break; }
                if matches!(command,Command::ReadProfiles) {
                    let result=(||->Result<FirmwareProfiles,String> {
                        if let Some(mut session)=live.take() { session.finish()?; }
                        if device.is_none() {
                            let path=hid::lighting_node().map_err(|e|e.to_string())?;
                            device=Some(hid::Device::open_for_write(path).map_err(|e|e.to_string())?);
                        }
                        let dev=device.as_ref().unwrap();
                        dev.ensure_lighting().map_err(|e|e.to_string())?;
                        read_profiles(dev)
                    })();
                    let message=match result {
                        Ok(profiles)=>{ let _=loaded_tx.send(profiles); "Read all three profiles from the keyboard firmware".to_string() },
                        Err(error)=>{device=None;format!("Could not read profiles: {error}")},
                    };
                    eprintln!("Legion Light: {message}");
                    if tx.send(message).is_err() { break; }
                    continue;
                }
                let configuring=matches!(command,Command::ConfigureProfiles(_));
                let start=Instant::now();
                let result=(||->Result<bool,String> {
                    if let Some(mut session)=live.take() { session.finish()?; }
                    let (reports,active)=match &command {
                        Command::Apply(state)=>(protocol::reports_for(state),state),
                        Command::ConfigureProfiles(set)=>(protocol::profile_reports(set),&set.profiles[set.selected]),
                        Command::ReadProfiles|Command::Shutdown=>unreachable!(),
                    };
                    let reports=reports.map_err(|e|e.to_string())?;
                    if device.is_none() {
                        let path=hid::lighting_node().map_err(|e|e.to_string())?;
                        device=Some(hid::Device::open_for_write(path).map_err(|e|e.to_string())?);
                    }
                    let dev=device.as_ref().unwrap();
                    dev.ensure_lighting().map_err(|e|e.to_string())?;
                    for (index,report) in reports.iter().enumerate() {
                        if index>0 { thread::sleep(protocol::REPORT_SPACING); }
                        dev.send(report).map_err(|e|e.to_string())?;
                    }
                    if active.enabled && active.effect==Effect::CustomCycle {
                        live=Some(palette::Session::begin(active.clone())?);
                        Ok(true)
                    }else{Ok(false)}
                })();
                let message=match result {
                    Ok(true)=>"Custom palette playing · keep the app open · first colour is the firmware fallback".into(),
                    Ok(false) if configuring=>"All three Fn+Space profiles saved".into(),
                    Ok(false)=>format!("Keyboard update sent in {} ms",start.elapsed().as_millis()),
                    Err(error)=>{device=None;format!("Not applied: {error}")},
                };
                eprintln!("Legion Light: {message}");
                if tx.send(message).is_err() { break; }
            }
            // Exit and window-close wait for this cleanup acknowledgement.
            if let Some(mut session)=live.take() {
                if let Err(error)=session.finish() { eprintln!("Legion Light cleanup: {error}"); }
            }
            let _=tx.send(SHUTDOWN_COMPLETE.into());
        });
        Self { commands,events,loaded }
    }
}

fn query(dev: &hid::Device, command: u8) -> Result<Vec<u8>, String> {
    dev.send(&protocol::query_report(command)).map_err(|e|e.to_string())?;
    thread::sleep(protocol::REPORT_SPACING);
    dev.get_feature(protocol::REPORT_ID, protocol::REPORT_LEN).map_err(|e|e.to_string())
}

/// The firmware answers a profile query only for the selected slot, so each slot is
/// selected in turn and the original selection is restored afterwards (even on failure).
fn read_profiles(dev: &hid::Device) -> Result<FirmwareProfiles, String> {
    let selected=protocol::parse_value(protocol::CMD_QUERY_SELECTED,&query(dev,protocol::CMD_QUERY_SELECTED)?)?;
    if !(1..=3).contains(&selected) { return Err(format!("firmware reports selected profile {selected}")); }
    let brightness=protocol::parse_value(protocol::CMD_QUERY_BRIGHTNESS,&query(dev,protocol::CMD_QUERY_BRIGHTNESS)?)?;
    let select=|slot:u8|->Result<(),String> {
        for report in protocol::select_reports(slot) {
            dev.send(&report).map_err(|e|e.to_string())?;
            thread::sleep(protocol::REPORT_SPACING);
        }
        Ok(())
    };
    let mut slots=Vec::new();
    let read=(||->Result<(),String> {
        for slot in 1..=3u8 {
            select(slot)?;
            let (got,settings)=protocol::parse_profile(&query(dev,protocol::CMD_QUERY_PROFILE)?)?;
            if got!=slot { return Err(format!("asked for profile {slot}, firmware returned {got}")); }
            slots.push(settings);
        }
        Ok(())
    })();
    let restored=select(selected);
    read?;
    restored?;
    Ok(FirmwareProfiles{selected:selected as usize-1,brightness,slots:slots.try_into().map_err(|_|"incomplete profile read".to_string())?})
}
