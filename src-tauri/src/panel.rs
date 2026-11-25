//! 底部面板：把 Tauri 窗口转成 NSPanel，在光标所在屏幕的底边全宽弹出。
//!
//! 为什么要 NSPanel：普通窗口拿键盘焦点就得把整个 App 激活到前台，原来的 App 就"失焦"了。
//! nonactivating panel 能接收键盘输入，但不激活我们的 App——之前的 App 仍是当前应用，
//! 面板一收起，模拟的 ⌘V 就会落到它里面。

// tauri_panel! 宏的事件声明语法要求写 `-> ()`
#![allow(clippy::unused_unit)]

use std::time::Duration;

use anyhow::anyhow;
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewWindow};
use tauri_nspanel::{
    tauri_panel, CollectionBehavior, ManagerExt, PanelLevel, StyleMask, WebviewWindowExt,
};

pub const MAIN_LABEL: &str = "main";
/// 面板即将显示，前端借此刷新列表。
pub const WILL_SHOW_EVENT: &str = "panel://will-show";
/// 面板已收起，前端趁看不见时把筛选、选中、滚动位置复位，下次弹出不闪。
pub const DID_HIDE_EVENT: &str = "panel://did-hide";

const SHEET_HEIGHT: f64 = 360.0;
/// 调整完位置尺寸后等一帧再显示，避免先闪一下旧布局。
const SHOW_DELAY: Duration = Duration::from_millis(16);

tauri_panel! {
    panel!(SheetPanel {
        config: {
            is_floating_panel: true,
            can_become_key_window: true,
            can_become_main_window: false
        }
    })

    panel_event!(SheetPanelEvents {
        window_did_resign_key(notification: &NSNotification) -> (),
    })
}

/// setup 阶段调用一次。
pub fn setup(app: &AppHandle) -> anyhow::Result<()> {
    let panel = main_window(app)?
        .to_panel::<SheetPanel>()
        .map_err(|err| anyhow!("convert window to panel failed: {err:?}"))?;

    // Dock 层级：面板显示时压在程序坞之上
    panel.set_level(PanelLevel::Dock.value());
    panel
        .set_style_mask(StyleMask::empty().nonactivating_panel().into())
        .map_err(|err| anyhow!("set panel style mask failed: {err:?}"))?;
    panel.set_collection_behavior(behavior(false).into());

    // 点到别的地方（面板失去键盘焦点）就收起
    let events = SheetPanelEvents::new();
    let handle = app.clone();
    events.window_did_resign_key(move |_| {
        if let Err(err) = hide(&handle) {
            log::warn!("hide panel on resign key failed: {err:#}");
        }
    });
    panel.set_event_handler(Some(events.as_ref()));

    Ok(())
}

/// 显示时 can_join_all_spaces：在当前桌面（包括全屏 App 上）出现；
/// 隐藏后切回 move_to_active_space：下次显示按当时所在桌面重新落位。
fn behavior(shown: bool) -> CollectionBehavior {
    let base = CollectionBehavior::new()
        .stationary()
        .full_screen_auxiliary();
    if shown {
        base.can_join_all_spaces()
    } else {
        base.move_to_active_space()
    }
}

pub fn show(app: &AppHandle) -> anyhow::Result<()> {
    place_at_bottom(&main_window(app)?)?;
    let _ = app.emit_to(MAIN_LABEL, WILL_SHOW_EVENT, ());

    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(SHOW_DELAY).await;
        let panel_handle = handle.clone();
        // NSPanel 的方法都必须在主线程调用
        let _ = handle.run_on_main_thread(move || {
            // 弹过确认框后 App 处于隐藏状态（见 native::run_alert），先解除，否则面板出不来
            crate::native::unhide_without_activation();
            if let Ok(panel) = panel_handle.get_webview_panel(MAIN_LABEL) {
                panel.show_and_make_key();
                panel.set_collection_behavior(behavior(true).into());
            }
        });
    });
    Ok(())
}

pub fn hide(app: &AppHandle) -> anyhow::Result<()> {
    let handle = app.clone();
    app.run_on_main_thread(move || {
        if let Ok(panel) = handle.get_webview_panel(MAIN_LABEL) {
            panel.hide();
            panel.set_collection_behavior(behavior(false).into());
        }
    })?;
    let _ = app.emit_to(MAIN_LABEL, DID_HIDE_EVENT, ());
    Ok(())
}

pub fn toggle(app: &AppHandle) -> anyhow::Result<()> {
    if main_window(app)?.is_visible()? {
        hide(app)
    } else {
        show(app)
    }
}

fn main_window(app: &AppHandle) -> anyhow::Result<WebviewWindow> {
    app.get_webview_window(MAIN_LABEL)
        .ok_or_else(|| anyhow!("main window not found"))
}

/// 定位到光标所在屏幕：全宽、贴底、固定高度。
///
/// 全程用逻辑坐标（macOS 的 point）计算。tao 给出的物理坐标里，光标是按「主屏」缩放比换算的，
/// 显示器位置尺寸是按「各自」缩放比换算的，先各自换回 point 再比较，混合 DPI 的多屏也不会找错屏。
fn place_at_bottom(window: &WebviewWindow) -> anyhow::Result<()> {
    let primary = window.primary_monitor()?;
    let primary_scale = primary.as_ref().map_or(1.0, |m| m.scale_factor());
    let cursor = window.cursor_position()?.to_logical::<f64>(primary_scale);

    let monitor = window
        .available_monitors()?
        .into_iter()
        .find(|m| {
            let scale = m.scale_factor();
            let pos = m.position().to_logical::<f64>(scale);
            let size = m.size().to_logical::<f64>(scale);
            (pos.x..pos.x + size.width).contains(&cursor.x)
                && (pos.y..pos.y + size.height).contains(&cursor.y)
        })
        .or(primary)
        .ok_or_else(|| anyhow!("no monitor found"))?;

    let scale = monitor.scale_factor();
    let pos = monitor.position().to_logical::<f64>(scale);
    let size = monitor.size().to_logical::<f64>(scale);
    let height = SHEET_HEIGHT.min(size.height);

    window.set_size(LogicalSize::new(size.width, height))?;
    window.set_position(LogicalPosition::new(pos.x, pos.y + size.height - height))?;
    Ok(())
}
