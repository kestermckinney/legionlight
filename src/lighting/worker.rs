//! Serialized profile writes and transient LampArray playback. Startup sends nothing.
use std::{sync::mpsc::{self, Receiver, Sender}, thread, time::{Duration, Instant}};
use super::{hid, palette, protocol};
use crate::model::{Effect, LightingState, ProfileSet};

pub const SHUTDOWN_COMPLETE: &str = "__legion_shutdown_complete__";
pub enum Command {
    Apply(LightingState),
    ConfigureProfiles(ProfileSet),
    Shutdown,
}
pub struct Worker { pub commands: Sender<Command>, pub events: Receiver<String> }
impl Worker {
    pub fn start() -> Self {
        let (commands, rx) = mpsc::channel::<Command>();
        let (tx, events) = mpsc::channel();
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
                let configuring=matches!(command,Command::ConfigureProfiles(_));
                let start=Instant::now();
                let result=(||->Result<bool,String> {
                    if let Some(mut session)=live.take() { session.finish()?; }
                    let (reports,active)=match &command {
                        Command::Apply(state)=>(protocol::reports_for(state),state),
                        Command::ConfigureProfiles(set)=>(protocol::profile_reports(set),&set.profiles[set.selected]),
                        Command::Shutdown=>unreachable!(),
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
        Self { commands,events }
    }
}
