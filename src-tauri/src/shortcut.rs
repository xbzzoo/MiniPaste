//! 全局快捷键：启动注册、修改快捷键窗口、录制期间暂停。
//!
//! 修改流程：打开窗口时先注销当前快捷键（否则按下它会直接弹面板，录不进去）→ 前端录到组合键后
//! 调 [`check`] 即时校验 → 保存时 [`change`] 注册新的，失败则保持旧配置 → 窗口销毁时按配置重新注册。

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use crate::config::Settings;
use crate::{menu, native, panel};

pub const DEFAULT: &str = "Shift+Cmd+V";
pub const WINDOW_LABEL: &str = "shortcut";

/// 系统或几乎所有 App 都在用的组合，被抢走会很难受，直接拒绝。
const RESERVED: [&str; 12] = [
    "Cmd+C",
    "Cmd+V",
    "Cmd+X",
    "Cmd+Z",
    "Shift+Cmd+Z",
    "Cmd+A",
    "Cmd+S",
    "Cmd+Q",
    "Cmd+W",
    "Cmd+Tab",
    "Cmd+Space",
    "Ctrl+Space",
];

/// 当前配置的快捷键是否注册成功。失败时菜单栏图标是唯一入口，不允许隐藏。
struct Registered(AtomicBool);

/// 启动时按配置注册。失败（被占用或格式不对）不阻断启动。
pub fn init(app: &AppHandle) {
    let accelerator = app.state::<Settings>().get().shortcut;
    let ok = match register(app, &accelerator) {
        Ok(()) => {
            log::info!("global shortcut registered: {accelerator}");
            true
        }
        Err(err) => {
            log::error!("register global shortcut {accelerator:?} failed: {err:#}");
            false
        }
    };
    app.manage(Registered(AtomicBool::new(ok)));
}

pub fn is_registered(app: &AppHandle) -> bool {
    app.try_state::<Registered>()
        .is_some_and(|r| r.0.load(Ordering::Relaxed))
}

fn set_registered(app: &AppHandle, ok: bool) {
    if let Some(r) = app.try_state::<Registered>() {
        r.0.store(ok, Ordering::Relaxed);
    }
}

fn register(app: &AppHandle, accelerator: &str) -> anyhow::Result<()> {
    let shortcuts = app.global_shortcut();
    shortcuts.unregister_all()?;
    shortcuts.on_shortcut(accelerator, |app, _shortcut, event| {
        if event.state != ShortcutState::Pressed {
            return;
        }
        if let Err(err) = panel::toggle(app) {
            log::error!("toggle panel failed: {err:#}");
        }
    })?;
    Ok(())
}

fn pause(app: &AppHandle) {
    if let Err(err) = app.global_shortcut().unregister_all() {
        log::warn!("pause global shortcut failed: {err}");
    }
}

/// 按当前配置重新注册。
fn resume(app: &AppHandle) {
    let accelerator = app.state::<Settings>().get().shortcut;
    let ok = match register(app, &accelerator) {
        Ok(()) => {
            log::info!("global shortcut resumed: {accelerator}");
            true
        }
        Err(err) => {
            log::error!("re-register {accelerator:?} failed: {err:#}");
            false
        }
    };
    set_registered(app, ok);
    menu::refresh_shortcut(app);
}

/// 校验录到的组合键，返回给用户看的错误说明。
pub fn check(accelerator: &str) -> Result<(), String> {
    let shortcut: Shortcut = accelerator
        .parse()
        .map_err(|_| "不支持这个按键，换一个试试".to_owned())?;

    let main_modifiers = Modifiers::SUPER | Modifiers::CONTROL | Modifiers::ALT;
    if !shortcut.mods.intersects(main_modifiers) && !is_function_key(shortcut.key) {
        return Err("需要包含 ⌘、⌃、⌥ 中的至少一个（F1～F20 可以单独使用）".to_owned());
    }
    let reserved = RESERVED.iter().any(|r| {
        r.parse::<Shortcut>()
            .is_ok_and(|s| s.mods == shortcut.mods && s.key == shortcut.key)
    });
    if reserved {
        return Err("这是系统常用快捷键，占用后会影响其他 App，换一个吧".to_owned());
    }
    Ok(())
}

fn is_function_key(key: Code) -> bool {
    use Code::*;
    matches!(
        key,
        F1 | F2
            | F3
            | F4
            | F5
            | F6
            | F7
            | F8
            | F9
            | F10
            | F11
            | F12
            | F13
            | F14
            | F15
            | F16
            | F17
            | F18
            | F19
            | F20
    )
}

/// 切换到新快捷键并写入配置。注册失败时不改配置，返回错误说明。
///
/// 只在修改窗口里调用：此时旧快捷键已暂停，失败后继续保持暂停，让用户接着录。
pub fn change(app: &AppHandle, accelerator: &str) -> Result<(), String> {
    check(accelerator)?;
    if let Err(err) = register(app, accelerator) {
        log::warn!("register {accelerator:?} failed: {err:#}");
        pause(app);
        return Err("注册失败，可能已被其他 App 占用，换一个试试".to_owned());
    }
    app.state::<Settings>()
        .update(|c| c.shortcut = accelerator.to_owned());
    set_registered(app, true);
    menu::refresh_shortcut(app);
    log::info!("global shortcut changed to {accelerator}");
    Ok(())
}

/// 打开（或聚焦）修改快捷键窗口。必须在主线程调用（菜单回调即主线程）。
pub fn open_window(app: &AppHandle) -> anyhow::Result<()> {
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        window.show()?;
        window.set_focus()?;
        return Ok(());
    }

    pause(app);
    let built = WebviewWindowBuilder::new(app, WINDOW_LABEL, WebviewUrl::App("index.html".into()))
        .title("修改快捷键")
        .inner_size(420.0, 248.0)
        .resizable(false)
        .minimizable(false)
        .maximizable(false)
        .always_on_top(true)
        .center()
        .build();
    let window = match built {
        Ok(window) => window,
        Err(err) => {
            resume(app);
            return Err(err.into());
        }
    };

    let handle = app.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::Destroyed = event {
            // 保存过就是注册新的，取消就是恢复旧的
            resume(&handle);
            native::yield_focus();
        }
    });
    // 菜单栏 App 平时不在前台，要先激活，窗口才会到最前并接收键盘
    window.set_focus()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_reasonable_shortcuts() {
        for s in [
            DEFAULT,
            "Alt+Space",
            "Ctrl+Shift+V",
            "Cmd+Backquote",
            "F5",
            "Shift+F13",
        ] {
            assert_eq!(check(s), Ok(()), "{s}");
        }
    }

    #[test]
    fn rejects_shortcuts_that_break_typing_or_system() {
        assert!(check("Shift+A").is_err(), "只有 Shift 会影响打字");
        assert!(check("V").is_err());
        assert!(check("Cmd+V").is_err(), "系统粘贴");
        assert!(check("Cmd+Shift+Z").is_err(), "修饰键顺序不同也要认出来");
        assert!(check("Cmd+IntlBackslash").is_err(), "解析不了的键");
    }
}
