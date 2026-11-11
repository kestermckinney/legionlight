//! SeeMyMac design tokens: Inter/13px, 32px controls, GNOME header, restrained chrome.
use gpui::{prelude::*, *};
use legion_light::model::{Effect, Rgb};
use crate::app::LightingApp;
use std::sync::{Arc, OnceLock};

pub const NATIVE_FRAME: bool = cfg!(any(target_os = "macos", target_os = "windows"));
const BG: u32 = 0x0D1117;
const TITLE: u32 = 0x010409;
const SURFACE: u32 = 0x161B22;
const SELECTED: u32 = 0x1F2630;
const BORDER: u32 = 0x30363D;
const TEXT: u32 = 0xE6EDF3;
const MUTED: u32 = 0x9198A1;
const ACCENT: u32 = 0x2F6FEB;
const FOCUS: u32 = 0x6CA4FF;
/// The application icon, embedded so it shows in the header without installed assets.
fn app_icon() -> Arc<Image> {
    static ICON: OnceLock<Arc<Image>> = OnceLock::new();
    ICON.get_or_init(|| Arc::new(Image::from_bytes(ImageFormat::Png, include_bytes!("../assets/legion-light-64.png").to_vec()))).clone()
}
fn button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    div().id(id).h(px(32.)).px(px(12.)).flex().items_center().justify_center()
        .rounded(px(6.)).border_1().border_color(rgb(BORDER)).cursor_pointer()
        .text_color(rgb(TEXT)).hover(|s| s.bg(rgb(SELECTED))).child(label.into())
}
fn label(text: impl Into<SharedString>) -> Div {
    div().text_color(rgb(MUTED)).text_size(px(12.)).child(text.into())
}
fn rgb_value(c: Rgb) -> u32 { ((c.r as u32)<<16)|((c.g as u32)<<8)|c.b as u32 }
impl LightingApp {
    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let control = |id, text| button(id, text).h(px(24.)).px(px(8.))
            .on_mouse_down(MouseButton::Left, |_,_,cx|cx.stop_propagation());
        div().id("titlebar").h(px(32.)).flex_none().px_2().flex().items_center().gap_2()
            .bg(rgb(TITLE)).border_b_1().border_color(rgb(BORDER))
            .on_mouse_down(MouseButton::Left, |event,window,_| {
                if event.click_count >= 2 { window.zoom_window(); } else { window.start_window_move(); }
            })
            .child(div().w(px(128.)).flex_none().flex().items_center()
                .child(img(app_icon()).id("app-icon").size(px(20.)).flex_none()))
            .child(div().flex_1().text_center().font_weight(FontWeight::SEMIBOLD).child("Legion Light"))
            .child(control("window-minimize","–").on_click(|_,window,_|window.minimize_window()))
            .child(control("window-maximize","□").on_click(|_,window,_|window.zoom_window()))
            .child(control("window-exit","Exit").debug_selector(|| "window-exit".into()).on_click(cx.listener(|app,_,_,cx|app.quit(cx))))
    }
    fn resize_handles(&self) -> impl IntoElement {
        let edge = |id: &'static str, e: ResizeEdge, cursor| div().id(id).absolute().cursor(cursor)
            .on_mouse_down(MouseButton::Left, move |_,window,_|window.start_window_resize(e));
        div()
            .child(edge("resize-t",ResizeEdge::Top,CursorStyle::ResizeUpDown).top_0().left(px(4.)).right(px(4.)).h(px(4.)))
            .child(edge("resize-b",ResizeEdge::Bottom,CursorStyle::ResizeUpDown).bottom_0().left(px(4.)).right(px(4.)).h(px(4.)))
            .child(edge("resize-l",ResizeEdge::Left,CursorStyle::ResizeLeftRight).left_0().top(px(4.)).bottom(px(4.)).w(px(4.)))
            .child(edge("resize-r",ResizeEdge::Right,CursorStyle::ResizeLeftRight).right_0().top(px(4.)).bottom(px(4.)).w(px(4.)))
            .child(edge("resize-tl",ResizeEdge::TopLeft,CursorStyle::ResizeUpLeftDownRight).top_0().left_0().size(px(4.)))
            .child(edge("resize-tr",ResizeEdge::TopRight,CursorStyle::ResizeUpRightDownLeft).top_0().right_0().size(px(4.)))
            .child(edge("resize-bl",ResizeEdge::BottomLeft,CursorStyle::ResizeUpRightDownLeft).bottom_0().left_0().size(px(4.)))
            .child(edge("resize-br",ResizeEdge::BottomRight,CursorStyle::ResizeUpLeftDownRight).bottom_0().right_0().size(px(4.)))
    }
    fn editor(&self, cx: &mut Context<Self>) -> Div {
        let mut content = div().flex().flex_col().gap_3()
            .child(div().flex().items_center().justify_between()
                .child(div().font_weight(FontWeight::SEMIBOLD).child(format!("Profile {}",self.profiles.selected+1)))
                .child(label(if self.state.enabled {self.state.effect.label()} else {"Off"})))
            .child(div().flex().flex_wrap().gap_2().children([
                ("static",Effect::Static),("breathe",Effect::Breathe),
                ("lenovo-colours",Effect::ColourCycle),("custom-palette",Effect::CustomCycle),
            ].into_iter().map(|(id,e)|button(id,e.label())
                .bg(rgb(if self.state.enabled && self.state.effect == e {ACCENT}else{SURFACE}))
                .on_click(cx.listener(move |app,_,_,cx|app.effect(e,cx)))))
                .child(button("off","Off").bg(rgb(if !self.state.enabled {ACCENT}else{SURFACE}))
                    .on_click(cx.listener(|app,_,_,cx|{app.state.enabled=false;app.apply(cx);}))))
            .child(label("One RGB zone · native effects keep running after the app closes"));
        if self.state.effect != Effect::Static {
            content = content.child(div().flex().items_center().gap_2().child(label("Speed"))
                .children([(1u8,"Slow"),(2,"Normal"),(3,"Fast")].into_iter().map(|(speed,text)|
                    button(("speed",speed as usize),text)
                        .bg(rgb(if self.state.effect_speed==speed {ACCENT}else{SURFACE}))
                        .on_click(cx.listener(move |app,_,_,cx|{app.state.effect_speed=speed;app.state.enabled=true;app.apply(cx);})))));
        }
        if self.state.effect == Effect::CustomCycle {
            content = content.child(label("App playback · keep the app running. Fn+Space stores the first colour."))
                .child(label("Palette · choose an entry, then edit its colour (2–4 entries)"))
                .child(div().flex().items_center().gap_2().children(self.state.palette.iter().enumerate().map(|(i,c)|
                    button(("palette",i),format!("{}",i+1)).bg(rgb(rgb_value(*c)))
                        .text_color(rgb(if u32::from(c.r)+u32::from(c.g)+u32::from(c.b)>380 {0x010409}else{0xffffff}))
                        .border_2().border_color(rgb(if self.palette_index==i {FOCUS}else{BORDER}))
                        .on_click(cx.listener(move |app,_,_,cx|{app.palette_index=i;app.sync_text();cx.notify();}))))
                    .when(self.state.palette.len()<4,|d|d.child(button("palette-add","+")
                        .on_click(cx.listener(|app,_,_,cx|app.palette_add(cx)))))
                    .when(self.state.palette.len()>2,|d|d.child(button("palette-remove","Remove")
                        .on_click(cx.listener(|app,_,_,cx|app.palette_remove(cx))))));
        }
        if self.state.effect != Effect::ColourCycle {
            let c=self.preview_colour();
            content=content.child(div().flex().items_center().gap_3()
                .child(div().size(px(32.)).rounded(px(6.)).border_1().border_color(rgb(BORDER)).bg(rgb(rgb_value(c))))
                .child(label(format!("{} · RGB {}, {}, {}",c.hex(),c.r,c.g,c.b))))
                .child(div().flex().flex_wrap().gap_2().children([
                    Rgb::new(255,0,0),Rgb::new(255,128,0),Rgb::new(255,220,0),Rgb::new(0,200,80),
                    Rgb::new(0,180,255),Rgb::new(40,80,255),Rgb::new(180,40,255),Rgb::WHITE,
                ].into_iter().enumerate().map(|(i,c)|div().id(("swatch",i)).size(px(32.)).rounded(px(6.))
                    .border_2().border_color(rgb(if self.preview_colour()==c {FOCUS}else{BORDER}))
                    .bg(rgb(rgb_value(c))).cursor_pointer()
                    .on_click(cx.listener(move |app,_,_,cx|app.colour(c,cx))))))
                .child(label("Custom colour · hex (#FF8000) or RGB (255,128,0)"))
                .child(div().flex().gap_2()
                    .child(div().id("custom-colour").debug_selector(|| "custom-colour".into()).h(px(32.)).flex_1().min_w_0().px_3()
                        .flex().items_center().rounded(px(6.)).bg(rgb(BG)).border_1()
                        .border_color(rgb(if self.editing {FOCUS}else{BORDER})).cursor_text().overflow_hidden()
                        .on_click(cx.listener(|app,_,window,cx|{app.editing=true;app.select_all=true;window.focus(&app.focus);cx.notify();}))
                        .child(div().bg(rgb(if self.editing && self.select_all {ACCENT}else{BG}))
                            .child(if self.editing && !self.select_all {
                                let mut text=self.colour_text.clone();text.insert(self.cursor,'│');text
                            }else{self.colour_text.clone()})))
                    .child(button("set-colour","Set colour").on_click(cx.listener(|app,_,_,cx|app.custom_colour(cx)))));
        } else {
            content=content.child(label("Lenovo’s built-in palette · choose Custom palette to use your own colours."));
        }
        content.child(label(format!("Brightness · {} / 9 (shared by the keyboard)",self.state.brightness)))
            .child(div().flex().flex_wrap().gap_2().children((1u8..=9).map(|step|
                button(("brightness",step as usize),step.to_string()).w(px(36.)).px_0()
                    .bg(rgb(if step==self.state.brightness {ACCENT}else{SURFACE}))
                    .on_click(cx.listener(move |app,_,_,cx|{app.state.brightness=step;app.state.enabled=true;app.apply(cx);})))) )
            .child(div().flex().gap_2().flex_wrap()
                .child(button("apply","Apply profile").bg(rgb(ACCENT))
                    .on_click(cx.listener(|app,_,_,cx|app.apply(cx))))
                .child(button("read-fw","Read profiles from keyboard")
                    .on_click(cx.listener(|app,_,_,cx|app.read_profiles(cx))))
                .child(button("save-fn","Save Fn+Space profiles")
                    .on_click(cx.listener(|app,_,_,cx|app.save_profiles(cx)))))
            .child(label("Profile tabs edit drafts. Changes apply to that slot; Save Fn+Space writes all three."))
            .child(div().border_t_1().border_color(rgb(BORDER)).pt_3().flex().flex_wrap().gap_2()
                .child(button("save-preset","Save to disk").on_click(cx.listener(|app,_,_,cx|app.preset(true,cx))))
                .child(button("load-preset","Load from disk").on_click(cx.listener(|app,_,_,cx|app.preset(false,cx))))
                .child(button("defaults","Lenovo defaults").on_click(cx.listener(|app,_,_,cx|app.defaults(cx)))))
            .child(label("Lenovo defaults replaces all three slots: changing colours, purple static, off."))
    }
}
impl Render for LightingApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut toolbar=div().h(px(40.)).flex_none().px_3().flex().items_center().gap_2()
            .border_b_1().border_color(rgb(BORDER));
        for i in 0..3 {
            toolbar=toolbar.child(button(("profile",i),format!("Profile {}",i+1))
                .bg(rgb(if !self.about && self.profiles.selected==i {ACCENT}else{BG}))
                .on_click(cx.listener(move |app,_,_,cx|{app.about=false;app.select_profile(i,cx);})))
        }
        toolbar=toolbar.child(div().flex_1()).child(button("about","About")
            .on_click(cx.listener(|app,_,_,cx|{app.about=!app.about;app.editing=false;cx.notify();})));
        let body=if self.about {
            div().flex().flex_col().gap_3().child("Lenovo Legion 5 · OneZone RGB")
                .child(label("048d:c615 · model 83Q7 · firmware profiles 1–3"))
                .child("Static colour and brightness are confirmed on this keyboard. Native pulse and hotkey profiles follow Lenovo’s Windows code. Custom palettes play through LampArray while the app stays open.")
                .child("Opening the app reads all three profiles from the firmware (briefly stepping through each slot, then restoring the active one) and shows them; it changes no stored settings. Applying saves the selected profile; Save Fn+Space and Lenovo defaults replace all three.")
                .child(label("Ctrl+Q exits · Tab focuses the colour field · Enter sets the colour"))
                .child(label("Window styling follows SeeMyMac’s design handoff and Linux header controls."))
        }else{self.editor(cx)};
        div().id("root").track_focus(&self.focus).size_full().relative().flex().flex_col()
            .rounded(px(if window.is_maximized() {0.}else{10.})).overflow_hidden()
            .bg(rgb(BG)).text_color(rgb(TEXT)).text_size(px(13.))
            .when_some(self.font.clone(),|d,font|d.font_family(font))
            .on_key_down(cx.listener(|app,event,window,cx|app.key_down(event,window,cx)))
            .when(!NATIVE_FRAME,|d|d.child(self.header(cx)))
            .when(NATIVE_FRAME,|d|d.child(button("native-exit","Exit").on_click(|_,_,cx|cx.quit())))
            .child(toolbar)
            .child(div().id("content-scroll").flex_1().min_h_0().overflow_y_scroll()
                .child(div().m_3().p_4().rounded(px(12.)).bg(rgb(SURFACE)).border_1().border_color(rgb(BORDER)).child(body)))
            .child(div().flex_none().border_t_1().border_color(rgb(BORDER)).px_4().py_2()
                .text_size(px(12.)).child(self.status.clone()))
            .when(!NATIVE_FRAME && !window.is_maximized(),|d|d.child(self.resize_handles()))
    }
}
