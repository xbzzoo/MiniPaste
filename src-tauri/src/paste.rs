//! 模拟 ⌘V，把内容粘贴进之前的前台 App。
//!
//! 需要「系统设置 → 隐私与安全性 → 辅助功能」授权；未授权时 CGEvent 会被系统静默丢弃。

use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::anyhow;
use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation, CGKeyCode};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

/// kVK_ANSI_V，与键盘布局无关的硬件键码。
const KEY_V: CGKeyCode = 0x09;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
}

/// 本次运行是否已经弹过系统的授权引导。
static PROMPTED: AtomicBool = AtomicBool::new(false);

/// 粘贴前检查辅助功能权限。未授权时只在本次运行第一次粘贴时弹系统引导，
/// 之后静默返回 false（内容已在剪贴板，用户手动 ⌘V 即可），不每次都打扰。
///
/// 授权是实时生效的：用户在设置里打开后，下一次粘贴就会返回 true。
pub fn ensure_trusted() -> bool {
    if is_trusted(false) {
        return true;
    }
    if !PROMPTED.swap(true, Ordering::Relaxed) {
        is_trusted(true);
    }
    false
}

/// 是否已有辅助功能权限。`prompt` 为 true 且未授权时，系统会弹窗引导用户去设置里打开
/// （每调用一次就弹一次，所以外部只通过 [`ensure_trusted`] 调用）。
fn is_trusted(prompt: bool) -> bool {
    unsafe {
        let key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
        let options = CFDictionary::from_CFType_pairs(&[(
            key.as_CFType(),
            CFBoolean::from(prompt).as_CFType(),
        )]);
        AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef())
    }
}

pub fn simulate_paste() -> anyhow::Result<()> {
    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
        .map_err(|_| anyhow!("create CGEventSource failed"))?;

    for key_down in [true, false] {
        let event = CGEvent::new_keyboard_event(source.clone(), KEY_V, key_down)
            .map_err(|_| anyhow!("create keyboard event failed"))?;
        event.set_flags(CGEventFlags::CGEventFlagCommand);
        event.post(CGEventTapLocation::HID);
    }
    Ok(())
}
