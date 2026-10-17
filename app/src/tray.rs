//! System tray icon with quick access to on/off, output mode, profile and quality.
//! Runs the Win32 message loop on the main thread.

use crate::state::{App, Command};
use aether_engine::{OutputMode, Quality};
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows_sys::Win32::UI::WindowsAndMessaging::{DispatchMessageW, GetMessageW, PostQuitMessage, SetTimer, TranslateMessage, MSG};

#[derive(Clone)]
enum Action {
    Toggle,
    Mode(OutputMode),
    Profile(String),
    Quality(Quality),
    Open,
    Autostart(bool),
    Quit,
}

/// Draws the tray icon: a disc with a center dot and sound-wave arcs on both sides.
fn make_icon(enabled: bool) -> Icon {
    const N: u32 = 32;
    let (bg, fg) = if enabled { ([20u8, 184, 166], [255u8, 255, 255]) } else { ([110u8, 110, 120], [225u8, 225, 230]) };
    let mut rgba = Vec::with_capacity((N * N * 4) as usize);
    let c = (N as f32 - 1.0) / 2.0;
    for y in 0..N {
        for x in 0..N {
            let (dx, dy) = (x as f32 - c, y as f32 - c);
            let r = (dx * dx + dy * dy).sqrt();
            let side = (dy.atan2(dx.abs())).abs() < 0.85;
            let ring = |radius: f32| (r - radius).abs() < 1.1 && side;
            let disc = (16.0 - r).clamp(0.0, 1.0);
            let on_fg = r < 3.6 || ring(8.0) || ring(12.5);
            let col = if on_fg { fg } else { bg };
            rgba.extend_from_slice(&[col[0], col[1], col[2], (disc * 255.0) as u8]);
        }
    }
    Icon::from_rgba(rgba, N, N).expect("icon")
}

fn mode_label(m: OutputMode) -> &'static str {
    match m {
        OutputMode::Headphones => "Headphones",
        OutputMode::Speakers => "Speakers",
        OutputMode::Stereo => "Stereo (no virtualization)",
    }
}

fn quality_label(q: Quality) -> &'static str {
    match q {
        Quality::Best => "Best sound",
        Quality::Balanced => "Balanced",
        Quality::Battery => "Best battery life",
    }
}

fn check(actions: &mut HashMap<MenuId, Action>, text: &str, checked: bool, action: Action) -> CheckMenuItem {
    let item = CheckMenuItem::new(text, true, checked, None);
    actions.insert(item.id().clone(), action);
    item
}

fn plain(actions: &mut HashMap<MenuId, Action>, text: &str, action: Action) -> MenuItem {
    let item = MenuItem::new(text, true, None);
    actions.insert(item.id().clone(), action);
    item
}

fn build_menu(app: &App) -> (Menu, HashMap<MenuId, Action>, String) {
    let cfg = app.config();
    let profile = App::active_profile(&cfg);
    let mut a = HashMap::new();
    let menu = Menu::new();

    let _ = menu.append(&MenuItem::new("Aether Sound", false, None));
    let _ = menu.append(&check(&mut a, "Enabled", cfg.enabled, Action::Toggle));
    let _ = menu.append(&PredefinedMenuItem::separator());

    let output = Submenu::new(format!("Output: {}", mode_label(cfg.mode)), true);
    for m in [OutputMode::Headphones, OutputMode::Speakers, OutputMode::Stereo] {
        let _ = output.append(&check(&mut a, mode_label(m), cfg.mode == m, Action::Mode(m)));
    }
    let _ = menu.append(&output);

    let profiles = Submenu::new(format!("Profile: {}", profile.name), true);
    let mut last_cat = String::new();
    for p in App::profiles_of(&cfg) {
        if !last_cat.is_empty() && p.category != last_cat {
            let _ = profiles.append(&PredefinedMenuItem::separator());
        }
        last_cat = p.category.clone();
        let _ = profiles.append(&check(&mut a, &p.name, p.id == cfg.profile_id, Action::Profile(p.id.clone())));
    }
    let _ = menu.append(&profiles);

    let quality = Submenu::new(format!("Quality: {}", quality_label(cfg.quality)), true);
    for q in [Quality::Best, Quality::Balanced, Quality::Battery] {
        let _ = quality.append(&check(&mut a, quality_label(q), cfg.quality == q, Action::Quality(q)));
    }
    let _ = menu.append(&quality);

    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&plain(&mut a, "Open Aether…", Action::Open));
    let _ = menu.append(&check(&mut a, "Start with Windows", cfg.autostart, Action::Autostart(!cfg.autostart)));
    let _ = menu.append(&plain(&mut a, "Quit", Action::Quit));
    let actions = a;

    let tooltip = if cfg.enabled { format!("Aether: {} · {}", mode_label(cfg.mode), profile.name) } else { "Aether: off".to_string() };
    (menu, actions, tooltip)
}

fn handle(app: &App, url: &str, action: Action) {
    let cfg = app.config();
    let cmd = match action {
        Action::Toggle => Command::SetEnabled { enabled: !cfg.enabled },
        Action::Mode(mode) => Command::SetMode { mode },
        Action::Profile(id) => Command::SelectProfile { id },
        Action::Quality(quality) => Command::SetQuality { quality },
        Action::Autostart(enabled) => Command::SetAutostart { enabled },
        Action::Open => return crate::platform::open_ui(url),
        Action::Quit => Command::Quit,
    };
    if let Err(e) = app.apply(cmd) {
        eprintln!("tray command failed: {e}");
    }
}

fn refresh(app: &App, tray: &TrayIcon, actions: &mut HashMap<MenuId, Action>) {
    let (menu, a, tooltip) = build_menu(app);
    *actions = a;
    tray.set_menu(Some(Box::new(menu)));
    let _ = tray.set_tooltip(Some(tooltip));
    let _ = tray.set_icon(Some(make_icon(app.config().enabled)));
}

/// Blocks until the user quits.
pub fn run(app: Arc<App>, url: String) -> anyhow::Result<()> {
    let (menu, mut actions, tooltip) = build_menu(&app);
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip(tooltip)
        .with_icon(make_icon(app.config().enabled))
        .build()?;
    let mut seen = app.version();

    unsafe {
        SetTimer(std::ptr::null_mut(), 0, 150, None);
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);

            while let Ok(ev) = MenuEvent::receiver().try_recv() {
                if let Some(a) = actions.get(&ev.id).cloned() {
                    handle(&app, &url, a);
                }
            }
            while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
                if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                    crate::platform::open_ui(&url);
                }
            }
            let v = app.version();
            if v != seen {
                seen = v;
                refresh(&app, &tray, &mut actions);
            }
            if app.quit.load(Ordering::Relaxed) {
                PostQuitMessage(0);
            }
        }
    }
    drop(tray);
    Ok(())
}
