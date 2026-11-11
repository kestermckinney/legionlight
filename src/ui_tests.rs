//! UI checks use a disconnected worker: no test sends HID reports.
use gpui::{Context, Window, TestAppContext, VisualTestContext, Modifiers, size, px};
use crate::app::LightingApp;
use legion_light::{lighting::worker::Worker, model::{Effect, Rgb}};

fn new_app(window: &mut Window, cx: &mut Context<LightingApp>) -> LightingApp {
    let mut app=LightingApp::new(window,cx);
    app.profiles=legion_light::model::ProfileSet::default();
    app.state=app.profiles.profiles[1].clone();
    app.sync_text();
    let (commands, receiver)=std::sync::mpsc::channel();
    drop(receiver);
    let (_,events)=std::sync::mpsc::channel();
    app.worker=Worker { commands,events,loaded: std::sync::mpsc::channel().1 };
    app
}
fn draw(cx: &mut VisualTestContext) { let _ = cx.update(|window,cx|window.draw(cx)); }
#[gpui::test]
fn compact_window_keeps_exit_and_colour_editor_visible(cx: &mut TestAppContext) {
    let (app,cx)=cx.add_window_view(new_app);
    for (w,h) in [(640.,640.),(560.,480.)] {
        cx.simulate_resize(size(px(w),px(h)));
        draw(cx);
        for selector in ["window-exit","custom-colour"] {
            let b=cx.debug_bounds(selector).expect("control painted");
            assert!(b.size.width>px(0.) && b.size.height>px(0.));
            assert!(b.origin.x>=px(0.) && b.origin.x+b.size.width<=px(w));
            assert!(b.origin.y>=px(0.) && b.origin.y+b.size.height<=px(h));
        }
    }
    let b=cx.debug_bounds("window-exit").unwrap();
    cx.simulate_click(b.center(),Modifiers::default());
    assert!(app.read_with(cx,|app,_|app.quit_requested));
}
#[gpui::test]
fn colour_entry_select_all_edit_and_enter(cx: &mut TestAppContext) {
    let (app,cx)=cx.add_window_view(new_app);
    draw(cx);
    let b=cx.debug_bounds("custom-colour").unwrap();
    cx.simulate_click(b.center(),Modifiers::default());
    cx.simulate_keystrokes("ctrl-a f f 8 0 0 0 enter");
    assert_eq!(app.read_with(cx,|app,_|app.state.zones[0]),Rgb::new(255,128,0));
    cx.simulate_keystrokes("ctrl-a 0 0 0 0 f f left backspace f end enter");
    assert_eq!(app.read_with(cx,|app,_|app.state.zones[0]),Rgb::new(0,0,255));
    cx.simulate_keystrokes("ctrl-q");
    assert!(app.read_with(cx,|app,_|app.quit_requested));
}
#[gpui::test]
fn draft_slots_and_palette_edits_stay_independent(cx: &mut TestAppContext) {
    let (app,cx)=cx.add_window_view(new_app);
    cx.update(|_,cx|app.update(cx,|app,cx| {
        app.select_profile(0,cx);
        app.effect(Effect::CustomCycle,cx);
        app.palette_add(cx);
        app.colour(Rgb::new(5,10,15),cx);
        assert_eq!(app.state.palette.len(),3);
        app.select_profile(1,cx);
        assert_eq!(app.state.effect,Effect::Static);
        app.colour(Rgb::new(90,80,70),cx);
        app.select_profile(0,cx);
        assert_eq!(app.state.effect,Effect::CustomCycle);
        assert_eq!(app.state.palette[2],Rgb::new(5,10,15));
        assert_eq!(app.profiles.profiles[1].zones[0],Rgb::new(90,80,70));
        assert_eq!(app.state.profile,Some(1));
        app.palette_index=2;
        app.palette_remove(cx);
        assert_eq!(app.state.palette.len(),2);
        assert_eq!(app.palette_index,1);
    }));
    draw(cx);
}

#[gpui::test]
fn exit_waits_for_worker_cleanup_acknowledgement(cx: &mut TestAppContext) {
    let (app,cx)=cx.add_window_view(new_app);
    let (commands,receive)=std::sync::mpsc::channel();
    let (send,events)=std::sync::mpsc::channel();
    cx.update(|_,cx|app.update(cx,|app,cx| {
        app.worker=Worker { commands,events,loaded: std::sync::mpsc::channel().1 };
        app.quit(cx);
        assert!(app.quit_requested);
        assert_eq!(app.status,"Returning control to the keyboard…");
        app.poll_worker(cx);
        assert_eq!(app.status,"Returning control to the keyboard…");
    }));
    assert!(matches!(receive.recv().unwrap(),legion_light::lighting::worker::Command::Shutdown));
    send.send(legion_light::lighting::worker::SHUTDOWN_COMPLETE.into()).unwrap();
    cx.update(|_,cx|app.update(cx,|app,cx| {
        app.poll_worker(cx);
        assert_eq!(app.status,"Keyboard playback stopped");
    }));
}
