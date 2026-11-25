#[cfg(not(target_os = "macos"))]
compile_error!("MiniPaste 目前只支持 macOS（底部面板基于 NSPanel，自动粘贴基于 CGEvent）");

mod autostart;
mod clipboard;
mod commands;
mod config;
mod db;
mod detect;
mod menu;
mod native;
mod panel;
mod paste;
mod shortcut;

use tauri::{Manager, RunEvent};

pub fn run() {
    tauri::Builder::default()
        // 不要 Tauri 默认的 App 菜单：否则在「修改快捷键」窗口里按 ⌘Q / ⌘W / ⌘H
        // 会直接退出、关窗、隐藏，而不是被录下来
        .enable_macos_default_menu(false)
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        // nspanel 插件必须在 to_panel 之前注册
        .plugin(tauri_nspanel::init())
        .invoke_handler(tauri::generate_handler![
            commands::list_items,
            commands::get_overview,
            commands::paste_item,
            commands::delete_item,
            commands::hide_panel,
            commands::show_menu,
            commands::get_shortcut,
            commands::check_shortcut,
            commands::set_shortcut,
            commands::close_window,
        ])
        .setup(|app| {
            // 不在程序坞显示图标，只留菜单栏图标
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle();
            app.manage(config::Settings::load(handle)?);
            app.manage(db::Db::open(handle)?);

            panel::setup(handle)?;
            shortcut::init(handle);
            menu::setup(handle)?;
            clipboard::start_watcher(handle.clone());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building MiniPaste")
        .run(|app, event| {
            // 菜单栏图标被隐藏后，再次打开 MiniPaste（访达 / 启动台 / 聚焦）把它找回来
            if let RunEvent::Reopen { .. } = event {
                menu::set_tray_visible(app, true);
            }
        });
}
