//! 前端可调用的命令。只做参数转发，逻辑在各模块里。
//!
//! 读写数据库的命令都是 async：同步命令跑在主线程上，查询再快也不该占着主线程。

use std::collections::BTreeMap;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

use crate::clipboard::{self, UPDATED_EVENT};
use crate::config::{Period, Settings};
use crate::db::{now_ms, Cursor, Db, Filter, ItemView};
use crate::{menu, native, panel, paste, shortcut};

type CmdResult<T> = Result<T, String>;

/// 单页条数上限，防止前端传个超大值把整表拉过来。
const MAX_PAGE: u32 = 200;

fn to_msg(err: anyhow::Error) -> String {
    log::error!("{err:#}");
    format!("{err:#}")
}

fn since(app: &AppHandle) -> i64 {
    app.state::<Settings>().get().period.since(now_ms())
}

/// 当前显示范围内的一页记录。`cursor` 为上一页最后一条，首页传 null。
#[tauri::command]
pub async fn list_items(
    app: AppHandle,
    filter: Filter,
    cursor: Option<Cursor>,
    limit: u32,
) -> CmdResult<Vec<ItemView>> {
    app.state::<Db>()
        .list(
            since(&app),
            filter,
            cursor.as_ref(),
            limit.clamp(1, MAX_PAGE),
        )
        .map_err(to_msg)
}

#[derive(Serialize)]
pub struct Overview {
    period: Period,
    counts: BTreeMap<&'static str, i64>,
}

/// 当前显示范围，以及范围内每个类型的条数。
#[tauri::command]
pub async fn get_overview(app: AppHandle) -> CmdResult<Overview> {
    let period = app.state::<Settings>().get().period;
    let counts = app
        .state::<Db>()
        .counts(period.since(now_ms()))
        .map_err(to_msg)?;
    Ok(Overview { period, counts })
}

#[tauri::command]
pub async fn delete_item(app: AppHandle, id: String) -> CmdResult<()> {
    app.state::<Db>().delete(&id).map_err(to_msg)?;
    menu::refresh_usage(&app);
    Ok(())
}

#[tauri::command]
pub fn hide_panel(app: AppHandle) -> CmdResult<()> {
    panel::hide(&app).map_err(to_msg)
}

/// 在面板上弹出设置菜单。async：弹出后主线程进入菜单的事件循环，不能在 IPC 回调里同步等它。
#[tauri::command]
pub async fn show_menu(app: AppHandle, x: f64, y: f64) -> CmdResult<()> {
    menu::popup_on_panel(&app, x, y).map_err(to_msg)
}

/// 提示音 → 写回剪贴板 → 收起面板 → 模拟 ⌘V。
///
/// 必须是 async 命令：同步命令跑在主线程上，sleep 会卡住主线程，面板就没机会真正收起。
#[tauri::command]
pub async fn paste_item(app: AppHandle, id: String) -> CmdResult<()> {
    if app.state::<Settings>().get().paste_sound {
        native::play_paste_sound(&app);
    }
    {
        let db = app.state::<Db>();
        let item = db
            .get(&id)
            .map_err(to_msg)?
            .ok_or_else(|| "这条记录已不存在".to_owned())?;
        clipboard::write_back(&item, db.images_dir()).map_err(to_msg)?;
        db.touch(&id).map_err(to_msg)?;
    }
    let _ = app.emit(UPDATED_EVENT, ());

    panel::hide(&app).map_err(to_msg)?;
    // hide 是派发到主线程异步执行的；不等一下，⌘V 会赶在面板交还键盘焦点之前被面板自己吃掉
    tokio::time::sleep(Duration::from_millis(60)).await;

    if paste::ensure_trusted() {
        paste::simulate_paste().map_err(to_msg)?;
    } else {
        log::info!("accessibility not granted; content copied, auto paste skipped");
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutInfo {
    current: String,
    default_value: &'static str,
}

#[tauri::command]
pub fn get_shortcut(app: AppHandle) -> ShortcutInfo {
    ShortcutInfo {
        current: app.state::<Settings>().get().shortcut,
        default_value: shortcut::DEFAULT,
    }
}

/// 录到组合键时即时校验，错误说明直接显示给用户。
#[tauri::command]
pub fn check_shortcut(accelerator: String) -> Result<(), String> {
    shortcut::check(&accelerator)
}

#[tauri::command]
pub fn set_shortcut(app: AppHandle, accelerator: String) -> Result<(), String> {
    shortcut::change(&app, &accelerator)
}

/// 关闭调用方自己的窗口（修改快捷键窗口用，免得给前端开放窗口权限）。
#[tauri::command]
pub fn close_window(window: WebviewWindow) -> CmdResult<()> {
    window.close().map_err(|err| to_msg(err.into()))
}
