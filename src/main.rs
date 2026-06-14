//! Markman - a block-based Markdown editor built with GPUI.
//!
//! Reads file paths from command-line arguments and opens one GPUI window per
//! file. With no arguments, a single empty window is created.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "macos")]
embed_plist::embed_info_plist!("../resources/macos/Info.plist");

use std::borrow::Cow;
use std::path::PathBuf;

use gpui::*;

mod app_icon;
mod app_identity;
mod app_menu;
mod app_visibility;
mod code_runner;
mod components;
mod config;
mod editor;
mod export;
mod input;
mod i18n;
mod layout;
mod net;
mod platform;
mod theme;
mod window_chrome;

use app_menu::{init as init_app_menu, open_editor_window};
use components::init_with_keybindings as init_editor;
use i18n::I18nManager;
use theme::ThemeManager;

struct VelotypeAssets;

impl AssetSource for VelotypeAssets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        match path {
            "icon/workspace/folder.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/folder.svg"
            )))),
            "icon/workspace/markdown.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/markdown.svg"
            )))),
            "icon/workspace/chevron-right.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/chevron-right.svg"
            )))),
            "icon/workspace/chevron-down.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/chevron-down.svg"
            )))),
            "icon/workspace/files.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/files.svg"
            )))),
            "icon/workspace/list-tree.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/list-tree.svg"
            )))),
            "icon/workspace/tags.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/tags.svg"
            )))),
            "icon/workspace/graph.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/graph.svg"
            )))),
            "icon/workspace/ai-chat.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/ai-chat.svg"
            )))),
            "icon/workspace/graph-repel.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/graph-repel.svg"
            )))),
            "icon/workspace/graph-physics.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/graph-physics.svg"
            )))),
            "icon/workspace/graph-uncross.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/graph-uncross.svg"
            )))),
            "icon/workspace/graph-fit.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/graph-fit.svg"
            )))),
            "icon/workspace/graph-reset.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/graph-reset.svg"
            )))),
            "icon/workspace/graph-popout.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/graph-popout.svg"
            )))),
            "icon/workspace/search.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/search.svg"
            )))),
            "icon/workspace/file-plus.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/file-plus.svg"
            )))),
            "icon/workspace/folder-plus.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/workspace/folder-plus.svg"
            )))),
            "icon/titlebar/chrome-close.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/titlebar/chrome-close.svg"
            )))),
            "icon/titlebar/chrome-minimize.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/titlebar/chrome-minimize.svg"
            )))),
            "icon/titlebar/chrome-maximize.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/titlebar/chrome-maximize.svg"
            )))),
            "icon/titlebar/chrome-restore.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/titlebar/chrome-restore.svg"
            )))),
            "icon/toolbar/bold.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/bold.svg"
            )))),
            "icon/toolbar/italic.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/italic.svg"
            )))),
            "icon/toolbar/heading-1.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/heading-1.svg"
            )))),
            "icon/toolbar/heading-2.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/heading-2.svg"
            )))),
            "icon/toolbar/heading-3.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/heading-3.svg"
            )))),
            "icon/toolbar/list-ordered.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/list-ordered.svg"
            )))),
            "icon/toolbar/list-bullet.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/list-bullet.svg"
            )))),
            "icon/toolbar/code.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/code.svg"
            )))),
            "icon/toolbar/square-code.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/square-code.svg"
            )))),
            "icon/toolbar/link.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/link.svg"
            )))),
            "icon/toolbar/quote.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/quote.svg"
            )))),
            "icon/toolbar/table.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/table.svg"
            )))),
            "icon/toolbar/view-source.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/view-source.svg"
            )))),
            "icon/toolbar/view-rendered.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/view-rendered.svg"
            )))),
            "icon/toolbar/zoom-in.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/zoom-in.svg"
            )))),
            "icon/toolbar/zoom-out.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/zoom-out.svg"
            )))),
            "icon/toolbar/auto-save.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/auto-save.svg"
            )))),
            "icon/toolbar/save.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/save.svg"
            )))),
            "icon/toolbar/copy.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/copy.svg"
            )))),
            "icon/toolbar/chevrons-down-up.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/chevrons-down-up.svg"
            )))),
            "icon/toolbar/chevrons-up-down.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/chevrons-up-down.svg"
            )))),
            "icon/toolbar/circle-play.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/circle-play.svg"
            )))),
            "icon/toolbar/circle-stop.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/circle-stop.svg"
            )))),
            "icon/toolbar/x.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/x.svg"
            )))),
            "icon/toolbar/chevron-down.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/chevron-down.svg"
            )))),
            "icon/toolbar/chevron-up.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/chevron-up.svg"
            )))),
            "icon/toolbar/search.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/search.svg"
            )))),
            "icon/toolbar/undo-2.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/undo-2.svg"
            )))),
            "icon/toolbar/redo-2.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/redo-2.svg"
            )))),
            "icon/toolbar/ellipsis-vertical.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/ellipsis-vertical.svg"
            )))),
            "icon/toolbar/square-check-big.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/square-check-big.svg"
            )))),
            "icon/toolbar/minus.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/minus.svg"
            )))),
            "icon/toolbar/image.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/image.svg"
            )))),
            "icon/toolbar/table-of-contents.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/table-of-contents.svg"
            )))),
            "icon/toolbar/workflow.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/workflow.svg"
            )))),
            "icon/toolbar/wand-sparkles.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/wand-sparkles.svg"
            )))),
            "icon/toolbar/list-collapse.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/list-collapse.svg"
            )))),
            "icon/toolbar/maximize-2.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/maximize-2.svg"
            )))),
            "icon/toolbar/circle-help.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/circle-help.svg"
            )))),
            "icon/toolbar/list-checks.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/list-checks.svg"
            )))),
            "icon/toolbar/globe.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/globe.svg"
            )))),
            "icon/toolbar/replace.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/replace.svg"
            )))),
            "icon/toolbar/list-plus.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/list-plus.svg"
            )))),
            "icon/toolbar/settings-2.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/settings-2.svg"
            )))),
            "icon/toolbar/sparkles.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/icon/toolbar/sparkles.svg"
            )))),
            _ => Ok(None),
        }
    }

    fn list(&self, _path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Parse command-line arguments
    let mut detach = false;
    let mut input_paths = Vec::new();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--version" | "-v" => {
                println!("markman {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--help" | "-h" => {
                println!(
                    "markman {} - Markman, a block-based Markdown memo app",
                    env!("CARGO_PKG_VERSION")
                );
                println!();
                println!("USAGE:");
                println!("    markman [OPTIONS] [FILES...]");
                println!();
                println!("OPTIONS:");
                println!("    -v, --version    Print version information");
                println!("    -h, --help       Print this help message");
                println!("    -d, --detach     Launch in background (non-blocking)");
                println!();
                println!("FILES:");
                println!("    One or more markdown files to open. If no files are specified,");
                println!("    opens an empty document.");
                return;
            }
            "--detach" | "-d" => {
                detach = true;
            }
            option if option.starts_with('-') => {
                eprintln!("Unknown option: {}", option);
                std::process::exit(1);
            }
            path => {
                input_paths.push(PathBuf::from(path));
            }
        }
        i += 1;
    }

    #[cfg(not(target_os = "macos"))]
    let _ = detach;

    // On macOS, detach from terminal if requested
    // TODO: Other platforms may also need to be adapted
    #[cfg(target_os = "macos")]
    if detach {
        use std::process::Command;

        // Re-launch the application in the background without the --detach flag
        let exe_path = std::env::current_exe().expect("Failed to get executable path");
        let non_detach_args: Vec<String> = args
            .iter()
            .filter(|arg| *arg != "--detach" && *arg != "-d")
            .cloned()
            .collect();

        Command::new(exe_path)
            .args(&non_detach_args[1..])
            .spawn()
            .expect("Failed to detach process");

        return;
    }

    let application = Application::new().with_assets(VelotypeAssets);
    platform::install_open_external_files(&application);

    application.run(move |cx: &mut App| {
            app_icon::install();
            let preferences = config::load_or_create_app_preferences().unwrap_or_else(|err| {
                eprintln!("failed to initialize app preferences: {err}");
                Default::default()
            });
            I18nManager::init_with_language_id(cx, &preferences.default_language_id);
            ThemeManager::init_with_theme_id(cx, &preferences.default_theme_id);
            net::install_http_client(cx);
            platform::init_document_gestures(cx);
            init_editor(cx, &preferences.keybindings);
            init_app_menu(cx);
            app_visibility::init(cx);

            if input_paths.is_empty() {
                platform::init_external_open_handling(cx, Some(preferences));
                return;
            }

            platform::init_external_open_handling(cx, None);

            for path in &input_paths {
                let absolute_path = if path.is_absolute() {
                    path.clone()
                } else {
                    match std::env::current_dir() {
                        Ok(cwd) => cwd.join(path),
                        Err(_) => path.clone(),
                    }
                };

                let markdown = match std::fs::read_to_string(&absolute_path) {
                    Ok(content) => {
                        if let Err(err) = config::record_recent_file(&absolute_path) {
                            eprintln!("failed to update recent file history: {err}");
                        }
                        content
                    }
                    Err(err) => {
                        eprintln!(
                            "failed to read '{}': {err}. opened as empty document.",
                            absolute_path.display()
                        );
                        String::new()
                    }
                };
                let handle = open_editor_window(cx, markdown, Some(absolute_path));
                app_menu::restore_last_workspace_folder(&handle, cx);
            }
            app_menu::install_menus(cx);
            cx.refresh_windows();
        });
}
