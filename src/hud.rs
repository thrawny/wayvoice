#[cfg(not(feature = "hud-ui"))]
use log::warn;

pub fn is_supported() -> bool {
    cfg!(feature = "hud-ui")
}

#[cfg(feature = "hud-ui")]
pub fn run_hud() {
    imp::run_hud();
}

#[cfg(not(feature = "hud-ui"))]
pub fn run_hud() {
    warn!("HUD UI not built. Re-run with: cargo run --features hud-ui -- hud");
}

#[cfg(feature = "hud-ui")]
pub fn run_hud_preview() {
    imp::run_hud_preview();
}

#[cfg(not(feature = "hud-ui"))]
pub fn run_hud_preview() {
    warn!("HUD UI not built. Re-run with: just hud-preview");
}

#[cfg(feature = "hud-ui")]
mod imp {
    use crate::ipc::socket_path;
    use gtk::cairo::Context;
    use gtk::glib::{self, ControlFlow};
    use gtk::prelude::*;
    use gtk::{Application, ApplicationWindow, DrawingArea, Orientation};
    use gtk4 as gtk;
    use gtk4_layer_shell as layer_shell;
    use layer_shell::LayerShell;
    use std::cell::{Cell, RefCell};
    use std::fs::OpenOptions;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;
    use std::time::Duration;

    const HUD_APP_ID_DAEMON: &str = "com.thrawny.wayvoice.hud";
    const HUD_APP_ID_PREVIEW: &str = "com.thrawny.wayvoice.hud.preview";
    const HUD_WIDTH: i32 = 260;
    const HUD_HEIGHT: i32 = 72;
    const STATUS_POLL_MS: u64 = 180;
    const WAVE_FRAME_MS: u64 = 33;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum HudMode {
        Daemon,
        Preview,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum HudState {
        Hidden,
        Recording,
        Transcribing,
    }

    impl HudState {
        fn from_status(status: &str) -> Self {
            match status {
                "recording" => Self::Recording,
                "transcribing" => Self::Transcribing,
                _ => Self::Hidden,
            }
        }
    }

    pub fn run_hud() {
        let Some(_lock) = acquire_hud_lock() else {
            return;
        };

        run_app(HudMode::Daemon);
    }

    pub fn run_hud_preview() {
        run_app(HudMode::Preview);
    }

    fn run_app(mode: HudMode) {
        let mut builder = Application::builder().application_id(match mode {
            HudMode::Daemon => HUD_APP_ID_DAEMON,
            HudMode::Preview => HUD_APP_ID_PREVIEW,
        });

        if mode == HudMode::Preview {
            builder = builder.flags(gtk::gio::ApplicationFlags::NON_UNIQUE);
        }

        let app = builder.build();
        app.connect_activate(move |app| build_ui(app, mode));
        let _ = app.run_with_args::<&str>(&["wayvoice-hud"]);
    }

    fn build_ui(app: &Application, mode: HudMode) {
        let window = ApplicationWindow::builder()
            .application(app)
            .title("wayvoice")
            .default_width(HUD_WIDTH)
            .default_height(HUD_HEIGHT)
            .resizable(false)
            .decorated(false)
            .build();
        window.set_hide_on_close(true);
        window.set_focusable(false);
        window.set_focus_on_click(false);
        window.set_can_target(false);
        apply_transparent_style(&window);
        configure_layer_shell(&window);

        let root = gtk::Box::new(Orientation::Vertical, 0);
        root.set_focusable(false);
        root.set_focus_on_click(false);
        root.set_margin_start(12);
        root.set_margin_end(12);
        root.set_margin_top(8);
        root.set_margin_bottom(8);

        let wave = DrawingArea::builder()
            .content_width(236)
            .content_height(48)
            .build();
        wave.set_focusable(false);
        wave.set_focus_on_click(false);

        root.append(&wave);
        window.set_child(Some(&root));

        let phase = Rc::new(Cell::new(0.0));
        let hud_state = Rc::new(RefCell::new(match mode {
            HudMode::Daemon => HudState::Hidden,
            HudMode::Preview => HudState::Recording,
        }));

        {
            let phase = phase.clone();
            let hud_state = hud_state.clone();
            wave.set_draw_func(move |_, cr, width, height| {
                draw_wave(cr, width, height, phase.get(), *hud_state.borrow());
            });
        }

        {
            let wave = wave.clone();
            let phase = phase.clone();
            glib::timeout_add_local(Duration::from_millis(WAVE_FRAME_MS), move || {
                phase.set((phase.get() + 0.17) % (std::f64::consts::TAU));
                wave.queue_draw();
                ControlFlow::Continue
            });
        }

        match mode {
            HudMode::Daemon => {
                let poll_window = window.clone();
                let hud_state = hud_state.clone();
                glib::timeout_add_local(Duration::from_millis(STATUS_POLL_MS), move || {
                    let state = query_daemon_status()
                        .map(|s| HudState::from_status(&s))
                        .unwrap_or(HudState::Hidden);

                    if *hud_state.borrow() != state {
                        *hud_state.borrow_mut() = state;
                    }

                    match state {
                        HudState::Hidden => poll_window.hide(),
                        _ => {
                            if !poll_window.is_visible() {
                                poll_window.show();
                            }
                        }
                    }

                    ControlFlow::Continue
                });

                window.hide();
            }
            HudMode::Preview => {
                window.show();
            }
        }
    }

    fn configure_layer_shell(window: &ApplicationWindow) {
        if !layer_shell::is_supported() {
            return;
        }

        window.init_layer_shell();
        window.set_namespace(Some("wayvoice"));
        window.set_layer(layer_shell::Layer::Overlay);
        window.set_keyboard_mode(layer_shell::KeyboardMode::None);

        // Place near bottom and let compositor choose active output.
        // We intentionally do not force a monitor; compositor focus/output policy
        // generally places this on the currently active monitor.
        window.set_anchor(layer_shell::Edge::Bottom, true);
        window.set_anchor(layer_shell::Edge::Left, false);
        window.set_anchor(layer_shell::Edge::Top, false);
        window.set_anchor(layer_shell::Edge::Right, false);
        window.set_margin(layer_shell::Edge::Bottom, 48);
    }

    fn apply_transparent_style(window: &ApplicationWindow) {
        let css = gtk::CssProvider::new();
        css.load_from_data(
            "
            window.wayvoice-hud,
            window.wayvoice-hud > box {
                background: transparent;
                box-shadow: none;
            }
            ",
        );

        window.add_css_class("wayvoice-hud");

        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &css,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
    }

    fn draw_wave(cr: &Context, width: i32, height: i32, phase: f64, state: HudState) {
        let width = width.max(1) as f64;
        let height = height.max(1) as f64;

        cr.set_source_rgba(0.07, 0.08, 0.10, 0.28);
        cr.rectangle(0.0, 0.0, width, height);
        let _ = cr.fill();

        let (r, g, b) = match state {
            HudState::Recording => (0.97, 0.33, 0.42),
            HudState::Transcribing => (0.38, 0.65, 1.0),
            HudState::Hidden => (0.36, 0.40, 0.45),
        };

        cr.set_source_rgba(r, g, b, 0.95);

        let bars = 18;
        let bar_w = (width / bars as f64 * 0.62).max(2.0);
        let step = width / bars as f64;

        for i in 0..bars {
            let x = i as f64 * step + ((step - bar_w) * 0.5);
            let center_bias = 1.0 - ((i as f64 / (bars - 1) as f64) - 0.5).abs() * 1.4;
            let center_bias = center_bias.max(0.22);

            let motion = match state {
                HudState::Recording => (phase + i as f64 * 0.41).sin().abs(),
                HudState::Transcribing => {
                    0.35 + (phase * 0.55 + i as f64 * 0.16).sin().abs() * 0.25
                }
                HudState::Hidden => 0.10,
            };

            let bar_h = (height * (0.12 + motion * 0.72 * center_bias)).max(3.0);
            let y = (height - bar_h) * 0.5;

            cr.rectangle(x, y, bar_w, bar_h);
        }

        let _ = cr.fill();
    }

    fn query_daemon_status() -> Option<String> {
        let mut stream = UnixStream::connect(socket_path()).ok()?;
        let _ = stream.set_write_timeout(Some(Duration::from_millis(120)));
        let _ = stream.set_read_timeout(Some(Duration::from_millis(120)));

        stream.write_all(b"status\n").ok()?;

        let mut line = String::new();
        let mut reader = BufReader::new(stream);
        reader.read_line(&mut line).ok()?;

        Some(line.trim().to_string())
    }

    struct HudLock {
        _file: std::fs::File,
        path: PathBuf,
    }

    impl Drop for HudLock {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    fn acquire_hud_lock() -> Option<HudLock> {
        let path = hud_lock_path();

        for _ in 0..2 {
            match OpenOptions::new().create_new(true).write(true).open(&path) {
                Ok(mut file) => {
                    let _ = writeln!(file, "{}", std::process::id());
                    return Some(HudLock { _file: file, path });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if !is_existing_lock_alive(&path) {
                        let _ = std::fs::remove_file(&path);
                        continue;
                    }
                    return None;
                }
                Err(_) => return None,
            }
        }

        None
    }

    fn hud_lock_path() -> PathBuf {
        std::env::var("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/tmp"))
            .join("wayvoice-hud.lock")
    }

    fn is_existing_lock_alive(path: &Path) -> bool {
        let Ok(raw) = std::fs::read_to_string(path) else {
            return false;
        };

        let Ok(pid) = raw.trim().parse::<u32>() else {
            return false;
        };

        Path::new("/proc").join(pid.to_string()).exists()
    }
}
