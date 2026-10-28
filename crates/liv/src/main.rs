//! `liv`: open Liv. `--version` says which build this is.

use std::sync::Arc;

use gpui_kit::*;
use liv::identity;

/// livtk's mark, made from the logo by `cargo xtask icons`.
const MARK: &[u8] = include_bytes!("../../../assets/brand/livtk-256.png");

actions!(liv, [Quit, Hide]);

/// What Liv shows until its shell exists (TAS-131): its mark and its name.
struct Welcome {
    mark: Arc<Image>,
}

impl Render for Welcome {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_4()
            .child(img(self.mark.clone()).size(px(96.)))
            .child(div().text_2xl().child(identity::NAME))
    }
}

fn main() {
    if std::env::args_os().nth(1).is_some_and(|a| a == "--version") {
        println!("liv {}", livtk_build::DESCRIBE);
        return;
    }
    liv::log_file::init();
    log::info!("starting liv {}", livtk_build::DESCRIBE);
    let application = gpui_kit::application();
    application.on_reopen(|cx| cx.activate(true));
    application.run(|cx| {
        livtk_ui::init(cx);
        // Before any window or notification, as GPUI asks.
        cx.set_app_identity(identity::APP_ID, identity::NAME);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_action(|_: &Hide, cx| cx.hide());
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-h", Hide, None),
        ]);
        cx.set_menus(vec![Menu::new(identity::NAME).items([
            MenuItem::action("Hide Liv", Hide),
            MenuItem::separator(),
            MenuItem::action("Quit Liv", Quit),
        ])]);
        let options = WindowOptions {
            app_id: Some(identity::DIR.into()),
            titlebar: Some(TitlebarOptions {
                title: Some(identity::NAME.into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let mark = Arc::new(Image::from_bytes(ImageFormat::Png, MARK.to_vec()));
        if let Err(e) = gpui_kit::open_window(options, cx, |_, cx| cx.new(|_| Welcome { mark })) {
            log::error!("could not open Liv's window: {e}");
            cx.quit();
            return;
        }
        cx.activate(true);
    });
}
