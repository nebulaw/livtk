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

/// livtk's mark, made from the logo by `cargo xtask icons`.
const MARK: &[u8] = include_bytes!("../../../assets/brand/livtk-256.png");

actions!(liv, [Quit, Hide, CheckForUpdates, RestartToUpdate]);

/// Where updating stands, said in one line until the shell's notice replaces it (TAS-131).
use updates::Status as Update;

/// What Liv shows until its shell exists (TAS-131): its mark, its name, and where updating stands.
struct Welcome {
    mark: Arc<Image>,
    update: Update,
    /// Whether a check is under way: one at a time.
    checking: bool,
    /// Restart to Update was asked while a check ran: it restarts once the check is done, never
    /// in the middle of one.
    restart_after_check: bool,
}

impl Welcome {
    /// Look for an update off the window's thread; `told` when the person asked, so "up to date"
    /// and failures are said (a quiet check says only what it found).
    fn check(&mut self, told: bool, cx: &mut Context<Self>) {
        if self.checking {
            return;
        }
        self.checking = true;
        // What is ready now, read before the line says "checking": a check keeps it as it is,
        // and only a newer build replaces it.
        let ready = match &self.update {
            Update::Ready(staged) => Some((**staged).clone()),
            _ => None,
        };
        if told && ready.is_none() {
            self.update = Update::Checking;
            cx.notify();
        }
        let asked = ready.clone();
        let found = cx.background_executor().spawn(async move {
            let folders = Folders::here().ok_or("no folders for Liv on this system")?;
            let settings = Settings::load(&settings::path_in(&folders.config));
            let channel = updates::channel(&settings);
            let source = updates::source(channel, &folders, updates::keys_for(channel));
            updates::check(
                &*source,
                &updates::http(),
                &folders,
                livtk_build::BUILD,
                asked.as_ref(),
            )
            .map_err(|e| e.to_string())
        });
        cx.spawn(async move |this, cx| {
            let found = found.await;
            let _ = this.update(cx, |w, cx| {
                w.checking = false;
                let restart = std::mem::take(&mut w.restart_after_check);
                // A check the person is waiting on (asked, or a Restart waiting) says what it found.
                match updates::after_check(found, ready, told || restart) {
                    Some(status) => w.update = status,
                    None => return,
                }
                if restart {
                    w.restart_to_update(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Hand over to the staged build and quit: it finishes once Liv has gone.
    fn restart_to_update(&mut self, cx: &mut Context<Self>) {
        if self.checking {
            self.restart_after_check = true;
            return;
        }
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
        let opened = gpui_kit::open_window(options, cx, |_, cx| {
            cx.new(|_| Welcome {
                mark,
                update: Update::Quiet,
                checking: false,
                restart_after_check: false,
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
        // Started by an update: say so only now that the window is open, so a build that can't
        // get this far is rolled back; then its notes, once.
        if updates::report_started(livtk_build::BUILD)
            && let Some(notes) =
                Folders::here().and_then(|f| updates::notes_after_update(&f, livtk_build::BUILD))
        {
            welcome.update(cx, |w, cx| {
                w.update = Update::Updated(livtk_build::BUILD, notes);
                cx.notify();
            });
        }
        let w = welcome.clone();
        cx.on_action(move |_: &CheckForUpdates, cx| w.update(cx, |w, cx| w.check(true, cx)));
        let w = welcome.clone();
        cx.on_action(move |_: &RestartToUpdate, cx| w.update(cx, |w, cx| w.restart_to_update(cx)));
        if installed {
            // What an earlier run left in staging goes first, then a check at start, then one
            // every few hours, quietly; in that order, so nothing is cleared under a check.
            let w = welcome.downgrade();
            cx.spawn(async move |cx| {
                cx.background_executor()
                    .spawn(async {
                        if let Some(f) = Folders::here() {
                            updates::tidy(&f);
                        }
                    })
                    .await;
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
