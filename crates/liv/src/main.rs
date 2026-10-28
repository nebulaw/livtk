//! `liv`: open Liv. `--version` says which build this is; `--identity` says who Liv is to the system
//! and where its update channel is, one `key=value` a line, for `cargo xtask liv` to build and
//! release it by (so Liv's identity is written in one place). `--finish-update …` is how a staged
//! build finishes an update, without a window (`updates::Finish`).

use std::sync::Arc;

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use liv::identity::{self, Folders};
use liv::settings::{self, Settings};
use liv::updates::{self, Finish};
use livtk_update::stage::Staged;

/// livtk's mark, made from the logo by `cargo xtask icons`.
const MARK: &[u8] = include_bytes!("../../../assets/brand/livtk-256.png");

actions!(liv, [Quit, Hide, CheckForUpdates, RestartToUpdate]);

/// Where updating stands, as the window says it in one line until the shell's notice replaces
/// it (TAS-131).
#[derive(Clone, Debug)]
enum Update {
    Quiet,
    Checking,
    UpToDate,
    Ready(Box<Staged>),
    Failed(String),
    /// This start followed an update: to build N, with its notes.
    Updated(u64, String),
}

/// What Liv shows until its shell exists (TAS-131): its mark, its name, and where updating stands.
struct Welcome {
    mark: Arc<Image>,
    update: Update,
    /// Whether a check is under way: one at a time.
    checking: bool,
}

impl Welcome {
    /// Look for an update off the window's thread; `told` when the person asked, so "up to date"
    /// and failures are said (a quiet check says only what it found).
    fn check(&mut self, told: bool, cx: &mut Context<Self>) {
        if self.checking {
            return;
        }
        self.checking = true;
        if told {
            self.update = Update::Checking;
            cx.notify();
        }
        let found = cx.background_executor().spawn(async move {
            let folders = Folders::here().ok_or("no folders for Liv on this system")?;
            let settings = Settings::load(&settings::path_in(&folders.config));
            let channel = updates::channel(&settings);
            let source = updates::source(channel, &folders, updates::keys());
            updates::check(&*source, &updates::http(), &folders, livtk_build::BUILD)
                .map_err(|e| e.to_string())
        });
        cx.spawn(async move |this, cx| {
            let found = found.await;
            let _ = this.update(cx, |w, cx| {
                w.checking = false;
                w.update = match (found, told) {
                    (Ok(Some(staged)), _) => Update::Ready(Box::new(staged)),
                    (Ok(None), true) => Update::UpToDate,
                    (Err(e), true) => Update::Failed(e),
                    (Err(e), false) => {
                        log::warn!("update check: {e}");
                        return;
                    }
                    (Ok(None), false) => return,
                };
                cx.notify();
            });
        })
        .detach();
    }

    /// Hand over to the staged build and quit: it finishes once Liv has gone.
    fn restart_to_update(&mut self, cx: &mut Context<Self>) {
        let Update::Ready(staged) = &self.update else {
            return;
        };
        let handed = std::env::current_exe()
            .map_err(|e| e.to_string())
            .and_then(|exe| {
                let folders = Folders::here().ok_or("no folders for Liv on this system")?;
                updates::install(staged, &exe, &folders)
            });
        match handed {
            Ok(()) => cx.quit(),
            Err(e) => {
                self.update = Update::Failed(e);
                cx.notify();
            }
        }
    }

    fn line(&self) -> Option<String> {
        Some(match &self.update {
            Update::Quiet => return None,
            Update::Checking => "Checking for updates…".into(),
            Update::UpToDate => "Liv is up to date.".into(),
            Update::Ready(s) => format!("Build {} is ready. Restart to update.", s.manifest.build),
            Update::Failed(e) => format!("Couldn't check for updates: {e}"),
            Update::Updated(build, notes) if notes.is_empty() => {
                format!("Updated to build {build}.")
            }
            Update::Updated(build, notes) => format!("Updated to build {build}. {notes}"),
        })
    }
}

impl Render for Welcome {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ready = matches!(self.update, Update::Ready(_));
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_4()
            .child(img(self.mark.clone()).size(px(96.)))
            .child(div().text_2xl().child(identity::NAME))
            .children(self.line().map(|line| {
                div()
                    .id("update-line")
                    .text_sm()
                    .when(ready, |d| {
                        d.cursor_pointer()
                            .on_click(cx.listener(|w, _, _, cx| w.restart_to_update(cx)))
                    })
                    .child(line)
            }))
    }
}

fn main() {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    match args.first().and_then(|a| a.to_str()) {
        Some("--version") => {
            println!("liv {}", livtk_build::DESCRIBE);
            return;
        }
        Some("--identity") => {
            println!("app_id={}", identity::APP_ID);
            println!("name={}", identity::NAME);
            if let Some(f) = Folders::here() {
                println!("updates={}", f.updates.display());
            }
            return;
        }
        Some(updates::FINISH) => {
            liv::log_file::init();
            let code = match Finish::parse(&args).ok_or("a malformed request".to_owned()) {
                Ok(f) => match f.run() {
                    Ok(outcome) => {
                        log::info!("update to build {}: {outcome:?}", f.build);
                        0
                    }
                    Err(e) => {
                        log::error!("update to build {}: {e}", f.build);
                        1
                    }
                },
                Err(e) => {
                    log::error!("--finish-update: {e}");
                    2
                }
            };
            std::process::exit(code);
        }
        _ => {}
    }
    liv::log_file::init();
    log::info!("starting liv {}", livtk_build::DESCRIBE);
    // Started by an update: say so first, so the update keeps this build.
    let updated = updates::report_started(livtk_build::BUILD)
        .then(Folders::here)
        .flatten()
        .and_then(|f| updates::notes_after_update(&f, livtk_build::BUILD));
    // Only an installed Liv looks for updates: one run from the build folder never does.
    let installed = std::env::current_exe()
        .ok()
        .and_then(|e| std::fs::canonicalize(e).ok())
        .is_some_and(|e| updates::bundle_of(&e).is_some());
    let application = gpui_kit::application();
    application.on_reopen(|cx| cx.activate(true));
    application.run(move |cx| {
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
            MenuItem::action("Check for Updates…", CheckForUpdates),
            MenuItem::action("Restart to Update", RestartToUpdate),
            MenuItem::separator(),
            MenuItem::action("Hide Liv", Hide),
            MenuItem::separator(),
            MenuItem::action("Quit Liv", Quit),
        ])]);
        let options = WindowOptions {
            app_id: Some(identity::SHORT.into()),
            titlebar: Some(TitlebarOptions {
                title: Some(identity::NAME.into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let mark = Arc::new(Image::from_bytes(ImageFormat::Png, MARK.to_vec()));
        let update = match updated {
            Some(notes) => Update::Updated(livtk_build::BUILD, notes),
            None => Update::Quiet,
        };
        let opened = gpui_kit::open_window(options, cx, |_, cx| {
            cx.new(|_| Welcome {
                mark,
                update,
                checking: false,
            })
        });
        let welcome = match opened {
            Ok((_, welcome)) => welcome,
            Err(e) => {
                log::error!("could not open Liv's window: {e}");
                cx.quit();
                return;
            }
        };
        let w = welcome.clone();
        cx.on_action(move |_: &CheckForUpdates, cx| w.update(cx, |w, cx| w.check(true, cx)));
        let w = welcome.clone();
        cx.on_action(move |_: &RestartToUpdate, cx| w.update(cx, |w, cx| w.restart_to_update(cx)));
        if installed {
            // Once at start, then every few hours, quietly.
            let w = welcome.downgrade();
            cx.spawn(async move |cx| {
                loop {
                    if w.update(cx, |w, cx| w.check(false, cx)).is_err() {
                        break;
                    }
                    cx.background_executor().timer(updates::EVERY).await;
                }
            })
            .detach();
        }
        cx.activate(true);
    });
}
