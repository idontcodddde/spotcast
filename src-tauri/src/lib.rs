mod commands;

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconEvent},
    Manager, WindowEvent,
};

use tauri_plugin_global_shortcut::{Code, Modifiers, ShortcutState};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let search_index = commands::launcher::SearchIndexState::new();

    tauri::Builder::default()
        .manage(search_index.clone())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_shortcuts(["alt+space"])
                .unwrap()
                .with_handler(|app, shortcut, event| {
                    if shortcut.matches(Modifiers::ALT, Code::Space)
                        && event.state() == ShortcutState::Pressed
                    {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            commands::launcher::open_path,
            commands::launcher::set_launcher_height,
            commands::launcher::run_command,
            commands::launcher::open_url,
            commands::launcher::add_bookmark,
            commands::launcher::remove_bookmark,
            commands::launcher::open_bookmarks_file,
            commands::launcher::hide_launcher,
            commands::global_search,
        ])
        .setup(move |app| {
            #[cfg(desktop)]
            {
                let app_handle = app.handle().clone();

                commands::launcher::initialize_search_index(&search_index, &app_handle)?;

                commands::launcher::start_search_indexer(search_index.clone(), app_handle);

                let window = app
                    .get_webview_window("main")
                    .expect("main window not found");

                let show_item =
                    MenuItem::with_id(app, "show", "Show Spotcast", true, None::<&str>)?;

                let quit_item =
                    MenuItem::with_id(app, "quit", "Quit Spotcast", true, None::<&str>)?;

                let menu = Menu::with_items(app, &[&show_item, &quit_item])?;

                let tray = app.tray_by_id("spotcast-tray").ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "spotcast-tray was not created",
                    )
                })?;

                tray.set_menu(Some(menu))?;

                tray.set_show_menu_on_left_click(false)?;

                tray.on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_focus();
                        }
                    }

                    "quit" => {
                        app.exit(0);
                    }

                    _ => {}
                });

                tray.on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();

                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_focus();
                        }
                    }
                });

                let focus_window = window.clone();

                window.on_window_event(move |event| {
                    if let WindowEvent::Focused(focused) = event {
                        if !focused {
                            let _ = focus_window.hide();
                        }
                    }
                });

                let close_window = window.clone();

                window.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = close_window.hide();
                    }
                });
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
