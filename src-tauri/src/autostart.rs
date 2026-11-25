//! 开机自动启动：用系统的 SMAppService（macOS 13+），即「系统设置 → 通用 → 登录项」管理的那一套，
//! 列表里显示 MiniPaste 的名字和图标，在哪边关掉都同步。
//!
//! 状态以系统为准，不存配置：用户可能直接在系统设置里关掉。

use objc2::runtime::AnyClass;
use objc2_service_management::{SMAppService, SMAppServiceStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// macOS 13 以下没有 SMAppService
    Unsupported,
    Enabled,
    Disabled,
    /// 已登记，但被用户在系统设置里关掉了，要去那边打开
    RequiresApproval,
}

fn available() -> bool {
    AnyClass::get(c"SMAppService").is_some()
}

pub fn status() -> Status {
    if !available() {
        return Status::Unsupported;
    }
    // SAFETY: 上面已确认 SMAppService 存在（macOS 13+）
    let status = unsafe { SMAppService::mainAppService().status() };
    match status {
        SMAppServiceStatus::Enabled => Status::Enabled,
        SMAppServiceStatus::RequiresApproval => Status::RequiresApproval,
        // NotRegistered / NotFound（开发模式没有 .app 包时是 NotFound）
        _ => Status::Disabled,
    }
}

pub fn set_enabled(enabled: bool) -> anyhow::Result<()> {
    anyhow::ensure!(available(), "开机自动启动需要 macOS 13 或更高版本");
    if !enabled && status() == Status::Disabled {
        return Ok(()); // 没登记过，注销会报错
    }
    // SAFETY: 已确认 SMAppService 存在
    let result = unsafe {
        let service = SMAppService::mainAppService();
        if enabled {
            service.registerAndReturnError()
        } else {
            service.unregisterAndReturnError()
        }
    };
    result.map_err(|err| anyhow::anyhow!("{}", err.localizedDescription()))
}

/// 打开「系统设置 → 通用 → 登录项」。
pub fn open_login_items_settings() {
    if available() {
        // SAFETY: 已确认 SMAppService 存在
        unsafe { SMAppService::openSystemSettingsLoginItems() };
    }
}
