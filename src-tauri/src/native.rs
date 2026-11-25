//! macOS 原生小部件：粘贴提示音、确认弹窗。

use objc2::MainThreadMarker;
use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSAlertStyle, NSApplication, NSSound};
use objc2_foundation::NSString;
use tauri::AppHandle;

/// 系统自带音效，见 /System/Library/Sounds。
const PASTE_SOUND: &str = "Pop";

/// 播放粘贴提示音（异步播放，不阻塞）。
pub fn play_paste_sound(app: &AppHandle) {
    let _ = app.run_on_main_thread(|| {
        if let Some(sound) = NSSound::soundNamed(&NSString::from_str(PASTE_SOUND)) {
            // 连续粘贴时上一声可能还没播完，不先 stop 的话 play 会直接返回 false
            sound.stop();
            sound.play();
        }
    });
}

/// 确认弹窗，返回是否点了确认。`destructive` 时确认按钮显示为红色。必须在主线程调用。
pub fn confirm(title: &str, message: &str, action: &str, destructive: bool) -> bool {
    run_alert(
        NSAlertStyle::Warning,
        title,
        message,
        &[action, "取消"],
        destructive,
    )
}

/// 只有一个「好」按钮的提示。必须在主线程调用。
pub fn inform(title: &str, message: &str) {
    run_alert(NSAlertStyle::Informational, title, message, &["好"], false);
}

fn run_alert(
    style: NSAlertStyle,
    title: &str,
    message: &str,
    buttons: &[&str],
    destructive: bool,
) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        log::error!("alert must be shown on the main thread");
        return false;
    };
    let app = NSApplication::sharedApplication(mtm);
    // 菜单栏 App 平时不在前台：不先激活，弹窗会被别的窗口挡住，也收不到回车 / esc
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);

    let alert = NSAlert::new(mtm);
    alert.setAlertStyle(style);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(message));
    for (index, title) in buttons.iter().enumerate() {
        let button = alert.addButtonWithTitle(&NSString::from_str(title));
        if index == 0 && destructive {
            button.setHasDestructiveAction(true);
        }
        // 系统只给英文标题「Cancel」自动绑 esc，中文「取消」要手动绑
        if index == 1 {
            button.setKeyEquivalent(&NSString::from_str("\u{1b}"));
        }
    }
    let confirmed = alert.runModal() == NSAlertFirstButtonReturn;
    yield_focus();
    confirmed
}

/// 弹窗、设置窗口关掉后把前台交还给之前的 App。否则我们一直占着前台，下次自动粘贴的 ⌘V
/// 会落到自己身上。隐藏状态在下次弹出面板前解除（[`unhide_without_activation`]）。必须在主线程调用。
pub fn yield_focus() {
    if let Some(mtm) = MainThreadMarker::new() {
        NSApplication::sharedApplication(mtm).hide(None);
    }
}

/// 解除 [`run_alert`] 留下的隐藏状态，但不激活 App。必须在主线程调用。
pub fn unhide_without_activation() {
    if let Some(mtm) = MainThreadMarker::new() {
        let app = NSApplication::sharedApplication(mtm);
        if app.isHidden() {
            app.unhideWithoutActivation();
        }
    }
}
