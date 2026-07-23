#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod settings;
mod update;

use std::path::PathBuf;

fn settings_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    use tauri::Manager;
    app.path()
        .app_config_dir()
        .map(|d| d.join("plot.json"))
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_settings(app: tauri::AppHandle) -> Result<settings::Settings, String> {
    Ok(settings::load(&settings_path(&app)?))
}

#[tauri::command]
fn save_settings(app: tauri::AppHandle, new_settings: settings::Settings) -> Result<(), String> {
    settings::save(&settings_path(&app)?, &new_settings).map_err(|e| e.to_string())
}

fn default_resourcepacks_path() -> PathBuf {
    let appdata = std::env::var("APPDATA").unwrap_or_default();
    PathBuf::from(appdata).join(".minecraft").join("resourcepacks")
}

fn scan_options() -> engine::ScanOptions {
    engine::ScanOptions {
        exclude: std::env::current_exe().ok().into_iter().collect(),
    }
}

/// Runs the scan off the main thread (a 1000-pack folder would freeze the
/// window's message loop) and streams progress events to the UI.
async fn scan_dir(window: tauri::Window, dir: PathBuf) -> engine::ScanReport {
    use tauri::Emitter;
    tauri::async_runtime::spawn_blocking(move || {
        engine::scan_with_progress(&dir, &scan_options(), &|p| {
            let _ = window.emit(
                "scan-progress",
                serde_json::json!({ "name": p.name, "index": p.index, "total": p.total }),
            );
        })
    })
    .await
    .unwrap_or_else(|_| engine::scan_with(std::path::Path::new(""), &Default::default()))
}

#[tauri::command]
async fn scan_default(window: tauri::Window) -> engine::ScanReport {
    scan_dir(window, default_resourcepacks_path()).await
}

#[tauri::command]
async fn scan_path(window: tauri::Window, path: String) -> engine::ScanReport {
    scan_dir(window, PathBuf::from(path)).await
}

fn plot_temp_dir() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe
        .parent()
        .ok_or_else(|| "no exe directory".to_string())?
        .to_path_buf();
    Ok(dir.join("plot_temp"))
}

#[tauri::command]
async fn process_packs(
    window: tauri::Window,
    path: String,
) -> Result<engine::ProcessReport, String> {
    use tauri::Emitter;
    let opts = engine::ProcessOptions {
        resourcepacks: PathBuf::from(&path),
        plot_temp: plot_temp_dir()?,
    };
    tauri::async_runtime::spawn_blocking(move || {
        engine::process_with_progress(&opts, &mut |ev| {
            let _ = window.emit(
                "process-progress",
                serde_json::json!({ "name": ev.name, "index": ev.index, "total": ev.total }),
            );
        })
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn check_locks(path: String, names: Vec<String>) -> Vec<String> {
    engine::probe_locked(std::path::Path::new(&path), &names)
}

#[tauri::command]
fn open_plot_temp() -> Result<(), String> {
    let dir = plot_temp_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::process::Command::new("explorer")
        .arg(&dir)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Opens a new Explorer window with the pack highlighted. raw_arg keeps the
/// /select,"path" shape intact — std quoting would mangle paths with spaces.
#[tauri::command]
fn reveal_pack(dir: String, name: String) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let target = std::path::Path::new(&dir).join(&name);
    std::process::Command::new("explorer")
        .raw_arg(format!("/select,\"{}\"", target.display()))
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Opens the pack with its default association (archiver for zips,
/// Explorer for folders) without spawning a visible cmd window.
#[tauri::command]
fn open_pack(dir: String, name: String) -> Result<(), String> {
    let target = std::path::Path::new(&dir).join(&name);
    open_path_shell(&target)
}

/// ShellExecuteW "open" — file association without a console flash.
#[cfg(windows)]
fn open_path_shell(path: &std::path::Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "shell32")]
    extern "system" {
        fn ShellExecuteW(
            hwnd: isize,
            lp_operation: *const u16,
            lp_file: *const u16,
            lp_parameters: *const u16,
            lp_directory: *const u16,
            n_show_cmd: i32,
        ) -> isize;
    }
    let wide = |s: &std::ffi::OsStr| {
        s.encode_wide().chain(std::iter::once(0)).collect::<Vec<u16>>()
    };
    let file = wide(path.as_os_str());
    let op: Vec<u16> = "open\0".encode_utf16().collect();
    // > 32 means success per ShellExecute docs
    let rc = unsafe {
        ShellExecuteW(
            0,
            op.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1, // SW_SHOWNORMAL
        )
    };
    if rc as usize > 32 {
        Ok(())
    } else {
        Err(format!("ShellExecute failed ({rc}) for {}", path.display()))
    }
}

#[cfg(not(windows))]
fn open_path_shell(path: &std::path::Path) -> Result<(), String> {
    Err(format!("open not supported on this platform: {}", path.display()))
}

/// Open http(s) / any URL with the system default handler (browser for https).
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("only http(s) URLs are allowed".into());
    }
    open_path_shell(std::path::Path::new(&url))
}

/// Startup update probe. `null` when already latest, no release, or soft 404.
/// Transport / timeout errors surface as Err — UI must stay silent either way.
#[tauri::command]
async fn check_for_update() -> Result<Option<update::UpdateInfo>, String> {
    let current = env!("CARGO_PKG_VERSION").to_string();
    tauri::async_runtime::spawn_blocking(move || update::check_latest(&current))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

const WEBVIEW2_DOWNLOAD: &str = "https://developer.microsoft.com/microsoft-edge/webview2/";

/// Friendly bilingual guidance instead of a cryptic crash on stripped-down
/// Windows installs (LTSC etc.) that ship without the WebView2 runtime.
fn ensure_webview2() -> bool {
    if tauri::webview_version().is_ok() {
        return true;
    }
    let text = "Plot 需要 Microsoft WebView2 运行时才能显示界面。\n\
                点击确定将打开官方下载页面，安装后重新运行 Plot。\n\n\
                Plot needs the Microsoft WebView2 Runtime to display its UI.\n\
                Press OK to open the official download page, then run Plot again.";
    let caption = "Plot — WebView2";
    #[cfg(windows)]
    {
        #[link(name = "user32")]
        extern "system" {
            fn MessageBoxW(hwnd: isize, text: *const u16, caption: *const u16, utype: u32) -> i32;
        }
        let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
        unsafe {
            MessageBoxW(0, wide(text).as_ptr(), wide(caption).as_ptr(), 0x40);
        }
    }
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", WEBVIEW2_DOWNLOAD])
        .spawn();
    false
}

fn main() {
    if !ensure_webview2() {
        return;
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            scan_default,
            scan_path,
            process_packs,
            check_locks,
            open_plot_temp,
            reveal_pack,
            open_pack,
            open_url,
            check_for_update,
            app_version,
            get_settings,
            save_settings
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
