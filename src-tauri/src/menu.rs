//! 菜单：菜单栏图标的菜单，以及面板右上角「⋯」弹出的同一套选项
//! （菜单栏图标隐藏后，面板里的「⋯」就是唯一入口）。
//!
//! 两个菜单共用同一批菜单项实例（muda 允许一个菜单项挂在多个菜单下，状态自动同步），
//! 勾选状态、占用空间的文字只改一处。

use tauri::image::Image;
use tauri::menu::{
    CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu,
};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, LogicalPosition, Manager, Wry};

use crate::autostart::{self, Status as AutostartStatus};
use crate::clipboard::UPDATED_EVENT;
use crate::config::{Config, Period, Settings};
use crate::db::{now_ms, Db};
use crate::{native, panel, shortcut};

/// 显示范围变了，前端回到第一页重新加载。
pub const SETTINGS_EVENT: &str = "settings://changed";

const TRAY_ID: &str = "main";
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// 按时间清理的档位：删除「最近一次使用早于 N 之前」的记录。
#[derive(Debug, Clone, Copy)]
enum Age {
    Day,
    Week,
    Month,
    All,
}

impl Age {
    const ALL: [Age; 4] = [Age::Day, Age::Week, Age::Month, Age::All];

    fn id(self) -> &'static str {
        match self {
            Age::Day => "clean:day",
            Age::Week => "clean:week",
            Age::Month => "clean:month",
            Age::All => "clean:all",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|age| age.id() == id)
    }

    /// 菜单文字写完整句子：末尾带「…」的写法会被看成文字没显示全。
    fn menu_label(self) -> &'static str {
        match self {
            Age::Day => "删除 24 小时前的记录",
            Age::Week => "删除一周前的记录",
            Age::Month => "删除一个月前的记录",
            Age::All => "删除全部记录",
        }
    }

    /// 用在「…的记录」前面的时间描述。
    fn describe(self) -> &'static str {
        match self {
            Age::Day => " 24 小时前",
            Age::Week => "一周前",
            Age::Month => "一个月前",
            Age::All => "全部",
        }
    }

    fn before(self, now: i64) -> i64 {
        match self {
            Age::Day => now - DAY_MS,
            Age::Week => now - 7 * DAY_MS,
            Age::Month => now - 30 * DAY_MS,
            Age::All => i64::MAX,
        }
    }
}

pub struct AppMenu {
    panel_menu: Menu<Wry>,
    periods: Vec<(Period, CheckMenuItem<Wry>)>,
    usage: MenuItem<Wry>,
    sound: CheckMenuItem<Wry>,
    show_tray: CheckMenuItem<Wry>,
    open: MenuItem<Wry>,
    autostart: CheckMenuItem<Wry>,
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let config = app.state::<Settings>().get();

    let period_title = MenuItem::with_id(app, "period", "显示范围", false, None::<&str>)?;
    let periods = Period::ALL
        .into_iter()
        .map(|period| {
            let id = format!("period:{}", period.id());
            let checked = period == config.period;
            CheckMenuItem::with_id(app, id, period.label(), true, checked, None::<&str>)
                .map(|item| (period, item))
        })
        .collect::<tauri::Result<Vec<_>>>()?;

    let usage = MenuItem::with_id(app, "usage", "已占用 …", false, None::<&str>)?;
    let clean_items = Age::ALL
        .into_iter()
        .map(|age| MenuItem::with_id(app, age.id(), age.menu_label(), true, None::<&str>))
        .collect::<tauri::Result<Vec<_>>>()?;
    let clean_separator = PredefinedMenuItem::separator(app)?;
    let clean = Submenu::with_id_and_items(
        app,
        "clean",
        "清理历史记录",
        true,
        &[
            &clean_items[0],
            &clean_items[1],
            &clean_items[2],
            &clean_separator,
            &clean_items[3],
        ],
    )?;

    let sound = CheckMenuItem::with_id(
        app,
        "sound",
        "粘贴时播放音效",
        true,
        config.paste_sound,
        None::<&str>,
    )?;
    let show_tray = CheckMenuItem::with_id(
        app,
        "tray",
        "在菜单栏显示图标",
        true,
        config.show_tray_icon,
        None::<&str>,
    )?;
    let change_shortcut = MenuItem::with_id(app, "shortcut", "修改快捷键", true, None::<&str>)?;
    let autostart_status = autostart::status();
    let supported = autostart_status != AutostartStatus::Unsupported;
    let autostart = CheckMenuItem::with_id(
        app,
        "autostart",
        if supported {
            "开机自动启动"
        } else {
            "开机自动启动（需 macOS 13 及以上）"
        },
        supported,
        autostart_status == AutostartStatus::Enabled,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "退出 MiniPaste", true, None::<&str>)?;
    let open = MenuItem::with_id(
        app,
        "open",
        open_label(&config.shortcut),
        true,
        None::<&str>,
    )?;

    // 分隔线各菜单各建一份，其余菜单项两边共用
    let common = |menu: &Menu<Wry>| -> tauri::Result<()> {
        menu.append(&period_title)?;
        for (_, item) in &periods {
            menu.append(item)?;
        }
        let rest: [&dyn IsMenuItem<Wry>; 10] = [
            &PredefinedMenuItem::separator(app)?,
            &usage,
            &clean,
            &PredefinedMenuItem::separator(app)?,
            &change_shortcut,
            &autostart,
            &sound,
            &show_tray,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ];
        menu.append_items(&rest)
    };

    let tray_menu = Menu::with_items(app, &[&open, &PredefinedMenuItem::separator(app)?])?;
    common(&tray_menu)?;
    let panel_menu = Menu::new(app)?;
    common(&panel_menu)?;

    let tray = TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(include_bytes!("../icons/tray.png"))?)
        .icon_as_template(true)
        .tooltip("MiniPaste")
        .menu(&tray_menu)
        // 用户可能在系统设置里改了登录项：鼠标移到图标上（点开菜单之前）就重新读一次
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Enter { .. } = event {
                sync_autostart(tray.app_handle());
            }
        })
        .build(app)?;
    tray.set_visible(config.show_tray_icon || !shortcut::is_registered(app))?;

    app.manage(AppMenu {
        panel_menu,
        periods,
        usage,
        sound,
        show_tray,
        open,
        autostart,
    });
    // 托盘菜单和面板弹出菜单的点击都走这一个全局回调（在主线程上执行）
    app.on_menu_event(handle_menu_event);
    refresh_usage(app);
    Ok(())
}

/// 在面板上弹出菜单，坐标是相对面板左上角的逻辑坐标。
pub fn popup_on_panel(app: &AppHandle, x: f64, y: f64) -> anyhow::Result<()> {
    let window = app
        .get_webview_window(panel::MAIN_LABEL)
        .ok_or_else(|| anyhow::anyhow!("main window not found"))?;
    sync_autostart(app);
    let menu = app.state::<AppMenu>();
    window.popup_menu_at(&menu.panel_menu, LogicalPosition::new(x, y))?;
    Ok(())
}

/// 重新计算占用空间并更新菜单文字。入库、删除、清理之后调用。
pub fn refresh_usage(app: &AppHandle) {
    // 剪贴板监听可能比菜单先跑起来
    let Some(menu) = app.try_state::<AppMenu>() else {
        return;
    };
    match app.state::<Db>().usage() {
        Ok(usage) => {
            let text = format!(
                "已占用 {} · {} 条",
                format_bytes(usage.bytes),
                format_count(usage.count)
            );
            if let Err(err) = menu.usage.set_text(text) {
                log::warn!("update usage menu item failed: {err}");
            }
        }
        Err(err) => log::warn!("compute storage usage failed: {err:#}"),
    }
}

/// 显示或隐藏菜单栏图标，并记住选择。
pub fn set_tray_visible(app: &AppHandle, visible: bool) {
    let settings = app.state::<Settings>();
    if settings.get().show_tray_icon == visible {
        return;
    }
    let config = settings.update(|c| c.show_tray_icon = visible);
    apply_tray_visibility(app, &config);
    sync_checks(app, &config);
}

/// 快捷键没注册上时强制显示图标，否则就没有入口打开面板了。
fn apply_tray_visibility(app: &AppHandle, config: &Config) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let visible = config.show_tray_icon || !shortcut::is_registered(app);
        if let Err(err) = tray.set_visible(visible) {
            log::error!("set tray visibility failed: {err}");
        }
    }
}

/// 快捷键变化（修改、注册失败、恢复）后，更新「打开面板」上显示的快捷键和图标显隐。
pub fn refresh_shortcut(app: &AppHandle) {
    let Some(menu) = app.try_state::<AppMenu>() else {
        return;
    };
    let config = app.state::<Settings>().get();
    if let Err(err) = menu.open.set_text(open_label(&config.shortcut)) {
        log::warn!("update open menu item failed: {err}");
    }
    apply_tray_visibility(app, &config);
}

fn open_label(accelerator: &str) -> String {
    format!("打开面板    {}", pretty_shortcut(accelerator))
}

fn handle_menu_event(app: &AppHandle, event: MenuEvent) {
    let id = event.id().as_ref();
    match id {
        "open" => {
            if let Err(err) = panel::show(app) {
                log::error!("show panel from menu failed: {err:#}");
            }
        }
        "quit" => app.exit(0),
        "sound" => {
            let config = app
                .state::<Settings>()
                .update(|c| c.paste_sound = !c.paste_sound);
            sync_checks(app, &config);
        }
        "tray" => toggle_tray(app),
        "autostart" => toggle_autostart(app),
        "shortcut" => {
            if let Err(err) = shortcut::open_window(app) {
                log::error!("open shortcut window failed: {err:#}");
            }
        }
        _ => {
            if let Some(period) = Period::ALL
                .into_iter()
                .find(|p| id.strip_prefix("period:") == Some(p.id()))
            {
                set_period(app, period);
            } else if let Some(age) = Age::from_id(id) {
                clean(app, age);
            }
        }
    }
}

/// 点击勾选项时 muda 已经自己翻转了勾选状态，这里统一按配置重新校正一遍
/// （单选组要取消其他项；用户取消操作时要翻回去）。
fn sync_checks(app: &AppHandle, config: &Config) {
    let menu = app.state::<AppMenu>();
    let results = menu
        .periods
        .iter()
        .map(|(period, item)| item.set_checked(*period == config.period))
        .chain([
            menu.sound.set_checked(config.paste_sound),
            menu.show_tray.set_checked(config.show_tray_icon),
        ]);
    for result in results {
        if let Err(err) = result {
            log::warn!("sync menu check state failed: {err}");
        }
    }
}

/// 开机自动启动的勾选状态以系统为准。
fn sync_autostart(app: &AppHandle) {
    let Some(menu) = app.try_state::<AppMenu>() else {
        return;
    };
    let enabled = autostart::status() == AutostartStatus::Enabled;
    if let Err(err) = menu.autostart.set_checked(enabled) {
        log::warn!("sync autostart menu item failed: {err}");
    }
}

fn toggle_autostart(app: &AppHandle) {
    let enable = autostart::status() != AutostartStatus::Enabled;
    match autostart::set_enabled(enable) {
        Ok(()) => log::info!("autostart {}", if enable { "enabled" } else { "disabled" }),
        Err(err) => {
            log::error!("set autostart {enable} failed: {err:#}");
            native::inform(
                "开机自动启动设置失败",
                &format!("{err}\n也可以在「系统设置 → 通用 → 登录项」里手动添加 MiniPaste。"),
            );
        }
    }
    // 之前在系统设置里被关掉过的，要用户去那边重新打开，App 自己改不了
    if enable && autostart::status() == AutostartStatus::RequiresApproval {
        let go = native::confirm(
            "需要在系统设置里打开",
            "MiniPaste 已加入登录项，但之前在系统设置里被关掉了，需要你在那边重新打开。",
            "去设置",
            false,
        );
        if go {
            autostart::open_login_items_settings();
        }
    }
    sync_autostart(app);
}

fn set_period(app: &AppHandle, period: Period) {
    let config = app.state::<Settings>().update(|c| c.period = period);
    sync_checks(app, &config);
    let _ = app.emit_to(panel::MAIN_LABEL, SETTINGS_EVENT, ());
}

fn toggle_tray(app: &AppHandle) {
    let settings = app.state::<Settings>();
    let show = !settings.get().show_tray_icon;
    if !show {
        let shortcut = pretty_shortcut(&settings.get().shortcut);
        let allowed = if shortcut::is_registered(app) {
            native::confirm(
                "隐藏菜单栏图标？",
                &format!(
                    "隐藏后按 {shortcut} 打开面板，点面板右上角的「⋯」可以重新显示图标。\n再次打开 MiniPaste 也会恢复图标。"
                ),
                "隐藏",
                false,
            )
        } else {
            native::inform(
                "现在不能隐藏菜单栏图标",
                &format!(
                    "快捷键 {shortcut} 没有注册成功（可能被别的 App 占用），隐藏后就没有入口打开面板了。\n可以先在菜单里「修改快捷键」换一个。"
                ),
            );
            false
        };
        if !allowed {
            sync_checks(app, &settings.get());
            return;
        }
    }
    set_tray_visible(app, show);
}

fn clean(app: &AppHandle, age: Age) {
    let db = app.state::<Db>();
    let before = age.before(now_ms());
    let (count, estimate) = match db.stats_before(before) {
        Ok(stats) => stats,
        Err(err) => {
            log::error!("count items to clean failed: {err:#}");
            return;
        }
    };
    if count == 0 {
        let message = match age {
            Age::All => "历史记录是空的。".to_owned(),
            _ => format!("没有{}的记录。", age.describe()),
        };
        native::inform("没有可清理的记录", &message);
        return;
    }

    // 全部删除时直接用实际占用；部分删除只能按记录大小估算
    let freed = match age {
        Age::All => db.usage().map_or(estimate, |u| u.bytes),
        _ => estimate,
    };
    let title = match age {
        Age::All => format!("删除全部 {} 条记录？", format_count(count)),
        _ => format!("删除{}的 {} 条记录？", age.describe(), format_count(count)),
    };
    let message = format!("大约释放 {}，删除后无法恢复。", format_bytes(freed));
    if !native::confirm(&title, &message, "删除", true) {
        return;
    }

    // VACUUM 可能要几百毫秒，别卡主线程
    let app = app.clone();
    std::thread::spawn(move || {
        match app.state::<Db>().delete_before(before) {
            Ok(deleted) => log::info!("cleaned {deleted} items ({})", age.describe().trim()),
            Err(err) => log::error!("clean history failed: {err:#}"),
        }
        let _ = app.emit(UPDATED_EVENT, ());
        refresh_usage(&app);
    });
}

/// 与访达一致按 1000 进位。
fn format_bytes(bytes: u64) -> String {
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = "B";
    for next in ["KB", "MB", "GB", "TB"] {
        value /= 1000.0;
        unit = next;
        if value < 1000.0 {
            break;
        }
    }
    if value < 100.0 {
        format!("{value:.1} {unit}")
    } else {
        format!("{value:.0} {unit}")
    }
}

fn format_count(count: i64) -> String {
    let digits = count.to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// `Shift+Cmd+V` → `⇧⌘V`，修饰键按 macOS 习惯排成 ⌃⌥⇧⌘。与前端 `shortcut.ts` 的显示规则一致。
pub fn pretty_shortcut(shortcut: &str) -> String {
    const ORDER: [(&str, &[&str]); 4] = [
        ("⌃", &["ctrl", "control"]),
        ("⌥", &["alt", "option"]),
        ("⇧", &["shift"]),
        (
            "⌘",
            &["cmd", "command", "super", "cmdorctrl", "commandorcontrol"],
        ),
    ];
    let tokens: Vec<String> = shortcut
        .split('+')
        .map(|t| t.trim().to_lowercase())
        .collect();
    let mut out: String = ORDER
        .iter()
        .filter(|(_, names)| tokens.iter().any(|t| names.contains(&t.as_str())))
        .map(|(symbol, _)| *symbol)
        .collect();
    for token in &tokens {
        if !ORDER
            .iter()
            .any(|(_, names)| names.contains(&token.as_str()))
        {
            out.push_str(&key_label(token));
        }
    }
    out
}

/// 主键的显示：KeyV → V，Digit1 → 1，ArrowLeft → ←，Backquote → `
fn key_label(token: &str) -> String {
    let token = token
        .strip_prefix("key")
        .or_else(|| token.strip_prefix("digit"))
        .unwrap_or(token);
    let symbol = match token {
        "arrowup" | "up" => "↑",
        "arrowdown" | "down" => "↓",
        "arrowleft" | "left" => "←",
        "arrowright" | "right" => "→",
        "enter" => "↩",
        "backspace" => "⌫",
        "delete" => "⌦",
        "escape" | "esc" => "⎋",
        "tab" => "⇥",
        "pageup" => "⇞",
        "pagedown" => "⇟",
        "home" => "↖",
        "end" => "↘",
        "backquote" => "`",
        "minus" => "-",
        "equal" => "=",
        "bracketleft" => "[",
        "bracketright" => "]",
        "backslash" => "\\",
        "semicolon" => ";",
        "quote" => "'",
        "comma" => ",",
        "period" => ".",
        "slash" => "/",
        _ => "",
    };
    if !symbol.is_empty() {
        return symbol.to_owned();
    }
    // 单个字母大写；Space、F5、Numpad1 之类首字母大写
    let mut chars = token.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_bytes_like_finder() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1_500), "1.5 KB");
        assert_eq!(format_bytes(12_345_678), "12.3 MB");
        assert_eq!(format_bytes(345_000_000), "345 MB");
        assert_eq!(format_bytes(2_100_000_000), "2.1 GB");
    }

    #[test]
    fn formats_count_with_separators() {
        assert_eq!(format_count(7), "7");
        assert_eq!(format_count(1234), "1,234");
        assert_eq!(format_count(1_234_567), "1,234,567");
    }

    #[test]
    fn pretty_prints_shortcuts() {
        assert_eq!(pretty_shortcut("Shift+Cmd+V"), "⇧⌘V");
        assert_eq!(pretty_shortcut("Cmd+Shift+KeyV"), "⇧⌘V");
        assert_eq!(pretty_shortcut("Alt+Space"), "⌥Space");
        assert_eq!(pretty_shortcut("Ctrl+Alt+Shift+Cmd+P"), "⌃⌥⇧⌘P");
        assert_eq!(pretty_shortcut("Ctrl+ArrowLeft"), "⌃←");
        assert_eq!(pretty_shortcut("Cmd+Backquote"), "⌘`");
        assert_eq!(pretty_shortcut("Alt+1"), "⌥1");
        assert_eq!(pretty_shortcut("F5"), "F5");
    }
}
