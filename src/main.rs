mod app;
mod view;
use gpui::{prelude::*, *};
fn main() {
    Application::new().run(|cx: &mut App| {
        cx.on_window_closed(|cx| { if cx.windows().is_empty() { cx.quit(); } }).detach();
        let bounds = Bounds::centered(None, size(px(640.), px(640.)), cx);
        cx.open_window(WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(560.), px(480.))),
            titlebar: Some(TitlebarOptions { title: Some("Legion Light".into()), appears_transparent: !view::NATIVE_FRAME, ..Default::default() }),
            window_decorations: Some(if view::NATIVE_FRAME { WindowDecorations::Server } else { WindowDecorations::Client }),
            window_background: if view::NATIVE_FRAME { WindowBackgroundAppearance::Opaque } else { WindowBackgroundAppearance::Transparent },
            app_id: Some("io.gitlab.kestermckinney.LegionLight".into()),
            ..Default::default()
        }, |window, cx| {
            let view = cx.new(|cx| app::LightingApp::new(window, cx));
            let weak = view.downgrade();
            window.on_window_should_close(cx, move |_,cx| {
                weak.update(cx, |app,cx| app.quit(cx)).is_err()
            });
            view
        }).expect("open lighting window");
        cx.activate(true);
    });
}

#[cfg(all(test, feature = "gui-tests"))]
mod ui_tests;
