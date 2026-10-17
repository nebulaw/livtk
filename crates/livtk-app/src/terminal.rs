//! A shell in a terminal, without a window: the login shell started in a pseudo-terminal in a
//! project's folder, and Alacritty's emulation of the screen it draws (its grid, scrollback,
//! selection and modes). The shell is read on Alacritty's own thread; what it says that the
//! window should know (it drew, its title changed, it exited) comes out of `Shell::spawn`'s
//! channel. Drawn by `ui::terminal`; how keys become what the shell is sent is `keys`.

pub mod colors;
pub mod keys;

use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::sync::Arc;

use alacritty_terminal::event::{Event, EventListener, Notify, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg, Notifier};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::tty;

pub use alacritty_terminal;

/// The lines kept above the screen.
pub const SCROLLBACK: usize = 10_000;

/// The terminal's size: its grid, and a cell's size in pixels (which some programs ask for).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub columns: usize,
    pub lines: usize,
    pub cell_width: u16,
    pub cell_height: u16,
}

impl Size {
    fn window(self) -> WindowSize {
        WindowSize {
            num_lines: u16::try_from(self.lines).unwrap_or(u16::MAX),
            num_cols: u16::try_from(self.columns).unwrap_or(u16::MAX),
            cell_width: self.cell_width,
            cell_height: self.cell_height,
        }
    }
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.lines
    }

    fn screen_lines(&self) -> usize {
        self.lines
    }

    fn columns(&self) -> usize {
        self.columns
    }
}

/// What the shell's side says, passed on to the window as it comes.
#[derive(Clone)]
pub struct Listener(async_channel::Sender<Event>);

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        // The window gone, nobody is listening: nothing to do.
        let _ = self.0.try_send(event);
    }
}

/// A running shell and the screen it draws.
pub struct Shell {
    term: Arc<FairMutex<Term<Listener>>>,
    sender: EventLoopSender,
    size: Size,
    /// What the shell last named its window.
    pub title: Option<String>,
    /// The shell has exited: what it drew stays, and nothing more is sent to it.
    pub exited: bool,
}

impl Shell {
    /// The login shell (`$SHELL`, else `/bin/zsh`) started in `dir`, `size` large, with the
    /// channel its events come out of.
    pub fn spawn(dir: &Path, size: Size) -> io::Result<(Shell, async_channel::Receiver<Event>)> {
        let program = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_owned());
        Shell::spawn_program(dir, size, &program, &["-l".to_owned()])
    }

    /// `program` with `args` started in `dir`, as `spawn` starts the shell.
    pub fn spawn_program(
        dir: &Path,
        size: Size,
        program: &str,
        args: &[String],
    ) -> io::Result<(Shell, async_channel::Receiver<Event>)> {
        let (tx, rx) = async_channel::unbounded();
        let listener = Listener(tx);
        let config = Config {
            scrolling_history: SCROLLBACK,
            ..Config::default()
        };
        let term = Arc::new(FairMutex::new(Term::new(config, &size, listener.clone())));
        let env = HashMap::from([
            ("TERM".to_owned(), "xterm-256color".to_owned()),
            ("COLORTERM".to_owned(), "truecolor".to_owned()),
            ("TERM_PROGRAM".to_owned(), "livtk".to_owned()),
        ]);
        let options = tty::Options {
            shell: Some(tty::Shell::new(program.to_owned(), args.to_vec())),
            working_directory: Some(dir.to_path_buf()),
            drain_on_exit: true,
            env,
        };
        let pty = tty::new(&options, size.window(), 0)?;
        let event_loop = EventLoop::new(term.clone(), listener, pty, true, false)?;
        let sender = event_loop.channel();
        event_loop.spawn();
        Ok((
            Shell {
                term,
                sender,
                size,
                title: None,
                exited: false,
            },
            rx,
        ))
    }

    /// Send the shell `bytes`, as if typed.
    pub fn write(&self, bytes: impl Into<std::borrow::Cow<'static, [u8]>>) {
        if !self.exited {
            Notifier(self.sender.clone()).notify(bytes);
        }
    }

    /// The terminal is `size` now: the screen reflows and the shell is told.
    pub fn resize(&mut self, size: Size) {
        if size == self.size || size.columns == 0 || size.lines == 0 {
            return;
        }
        self.size = size;
        self.term.lock().resize(size);
        let _ = self.sender.send(Msg::Resize(size.window()));
    }

    pub fn size(&self) -> Size {
        self.size
    }

    /// The screen's text, a line each, trailing spaces trimmed.
    pub fn screen_text(&self) -> Vec<String> {
        self.with_term(|t| {
            let grid = t.grid();
            (0..grid.screen_lines())
                .map(|l| {
                    let row = &grid[alacritty_terminal::index::Line(i32::try_from(l).unwrap_or(0))];
                    let text: String = (0..grid.columns())
                        .map(|c| row[alacritty_terminal::index::Column(c)].c)
                        .collect();
                    text.trim_end().to_owned()
                })
                .collect()
        })
    }

    /// Work with the screen, the shell's thread kept waiting meanwhile.
    pub fn with_term<R>(&self, f: impl FnOnce(&mut Term<Listener>) -> R) -> R {
        f(&mut self.term.lock())
    }

    /// Take in what the shell said: its title, its exit. Whether the window should draw again.
    pub fn take(&mut self, event: &Event) -> bool {
        match event {
            Event::Title(title) => self.title = Some(title.clone()),
            Event::ResetTitle => self.title = None,
            Event::ChildExit(_) | Event::Exit => self.exited = true,
            // What the screen answers a program that asks (its size, a colour, the cursor's
            // place) goes back to it.
            Event::PtyWrite(text) => self.write(text.clone().into_bytes()),
            Event::TextAreaSizeRequest(answer) => {
                let text = answer(self.size.window());
                self.write(text.into_bytes());
            }
            // A program asking what a colour is (the background, to pick a light or dark theme).
            Event::ColorRequest(index, answer) => {
                let c = self.with_term(|t| colors::at(*index, t.colors()));
                let rgb = alacritty_terminal::vte::ansi::Rgb {
                    r: (c >> 16) as u8,
                    g: (c >> 8) as u8,
                    b: c as u8,
                };
                self.write(answer(rgb).into_bytes());
            }
            _ => {}
        }
        true
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.sender.send(Msg::Shutdown);
    }
}
