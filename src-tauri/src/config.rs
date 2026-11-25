//! 用户配置：`<app_config_dir>/config.json`，首次启动写入默认值。
//!
//! - `shortcut` / `maxItems`：手改文件，重启生效
//! - `period` / `showTrayIcon` / `pasteSound`：在菜单里切换，立即生效并写回文件

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const CONFIG_FILE: &str = "config.json";

const HOUR_MS: i64 = 60 * 60 * 1000;
const DAY_MS: i64 = 24 * HOUR_MS;

/// 面板显示多长时间内的记录（按「最近一次复制或粘贴」的时间算）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Period {
    Day,
    Week,
    All,
}

impl Period {
    pub const ALL: [Period; 3] = [Period::Day, Period::Week, Period::All];

    pub fn id(self) -> &'static str {
        match self {
            Period::Day => "day",
            Period::Week => "week",
            Period::All => "all",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Period::Day => "最近 24 小时",
            Period::Week => "最近一周",
            Period::All => "全部",
        }
    }

    /// 起始时间戳（毫秒），`All` 为 0。
    pub fn since(self, now_ms: i64) -> i64 {
        match self {
            Period::Day => now_ms - DAY_MS,
            Period::Week => now_ms - 7 * DAY_MS,
            Period::All => 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Config {
    /// 呼出面板的全局快捷键，语法同 Tauri global-shortcut，如 `Shift+Cmd+V`、`Alt+Space`。
    pub shortcut: String,
    /// 最多保留多少条历史，超出按最近使用时间淘汰。
    pub max_items: usize,
    /// 面板显示范围，默认最近 24 小时。
    pub period: Period,
    /// 是否在菜单栏显示图标。
    pub show_tray_icon: bool,
    /// 粘贴（回车 / 双击）时播放提示音。
    pub paste_sound: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            shortcut: "Shift+Cmd+V".to_owned(),
            max_items: 10_000,
            period: Period::Day,
            show_tray_icon: true,
            paste_sound: true,
        }
    }
}

/// 运行期配置。菜单改动通过 [`Settings::update`] 写回文件。
pub struct Settings {
    path: PathBuf,
    config: Mutex<Config>,
    /// 用户文件解析失败时为 false：此时只改内存，不覆盖用户的文件。
    writable: bool,
}

impl Settings {
    /// 读取配置；文件不存在时写入默认配置，解析失败时回退默认值。
    pub fn load(app: &AppHandle) -> anyhow::Result<Self> {
        let dir = app.path().app_config_dir()?;
        fs::create_dir_all(&dir).with_context(|| format!("create config dir {dir:?}"))?;
        let path = dir.join(CONFIG_FILE);

        let (config, writable) = if path.exists() {
            let raw = fs::read_to_string(&path).with_context(|| format!("read config {path:?}"))?;
            match serde_json::from_str::<Config>(&raw) {
                Ok(config) => (config, true),
                Err(err) => {
                    log::warn!("invalid config at {path:?}, falling back to defaults: {err}");
                    (Config::default(), false)
                }
            }
        } else {
            (Config::default(), true)
        };

        let settings = Self {
            path,
            config: Mutex::new(config),
            writable,
        };
        // 补齐新版本加的字段，用户打开文件就能看到所有可配置项
        settings.save(&settings.lock());
        Ok(settings)
    }

    pub fn get(&self) -> Config {
        self.lock().clone()
    }

    /// 修改并写回文件，返回修改后的配置。
    pub fn update(&self, change: impl FnOnce(&mut Config)) -> Config {
        let mut config = self.lock();
        change(&mut config);
        self.save(&config);
        config.clone()
    }

    fn lock(&self) -> MutexGuard<'_, Config> {
        self.config.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn save(&self, config: &Config) {
        if !self.writable {
            return;
        }
        let result = serde_json::to_string_pretty(config)
            .map_err(anyhow::Error::from)
            .and_then(|json| fs::write(&self.path, json).map_err(Into::into));
        if let Err(err) = result {
            log::error!("save config to {:?} failed: {err:#}", self.path);
        }
    }
}
