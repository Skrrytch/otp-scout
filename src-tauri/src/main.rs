use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{
    async_runtime,
    image::Image,
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    webview::WebviewWindowBuilder,
    AppHandle, Emitter, Manager, RunEvent, State, WebviewUrl,
};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_notification::NotificationExt;
use tokio::sync::{mpsc, Mutex, Notify};

mod api;
mod config;
mod html;
mod imap;
mod rules;
mod status;

use config::{AccountConfig, SharedConfig};

struct ImapTask {
    stop_tx: tokio::sync::oneshot::Sender<()>,
}

struct AppState {
    config: SharedConfig,
    tasks: Mutex<Vec<ImapTask>>,
    /// Wakes all IDLE loops for an immediate mailbox check.
    check_now: Arc<Notify>,
}

#[tauri::command]
fn get_accounts(state: State<AppState>) -> Vec<AccountConfig> {
    config::get_accounts(&state.config)
}

#[tauri::command]
fn add_account(state: State<AppState>, account: AccountConfig) -> Result<Vec<AccountConfig>, String> {
    config::add_account(&state.config, account).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn update_account(
    state: State<AppState>,
    account: AccountConfig,
) -> Result<Vec<AccountConfig>, String> {
    config::update_account(&state.config, account).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn remove_account(state: State<AppState>, id: String) -> Result<Vec<AccountConfig>, String> {
    config::remove_account(&state.config, &id).map_err(|e| e.to_string())
}

/// Immediate IMAP check: wakes running connections, reconnects failed ones.
#[tauri::command]
async fn check_now(app: AppHandle) -> Result<(), String> {
    trigger_check(app).await.map_err(|e| e.to_string())
}

async fn trigger_check(app: AppHandle) -> anyhow::Result<()> {
    // Wakes idling connections and skips the backoff of failed ones.
    app.state::<AppState>().check_now.notify_waiters();
    Ok(())
}

#[tauri::command]
fn get_app_info(app: AppHandle) -> serde_json::Value {
    serde_json::json!({
        "name": "OTP-Scout",
        "tagline": "your one-time password helper",
        "version": app.package_info().version.to_string(),
    })
}

#[derive(serde::Serialize)]
struct ApiSettings {
    config: config::ApiConfig,
    token: String,
    status: api::ApiStatus,
}

#[tauri::command]
fn get_api_settings(state: State<AppState>, server: State<api::ApiServer>) -> Result<ApiSettings, String> {
    Ok(ApiSettings {
        config: config::get_api_config(&state.config),
        token: config::api_token().map_err(|e| format!("{e:#}"))?,
        status: server.status(),
    })
}

#[tauri::command]
async fn set_api_settings(app: AppHandle, api: config::ApiConfig) -> Result<(), String> {
    let shared = app.state::<AppState>().config.clone();
    tauri::async_runtime::spawn_blocking(move || config::set_api_config(&shared, api))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("{e:#}"))?;
    api::restart(app).await;
    Ok(())
}

#[tauri::command]
fn regenerate_api_token(server: State<api::ApiServer>) -> Result<String, String> {
    let token = config::regenerate_api_token().map_err(|e| format!("{e:#}"))?;
    server.set_token(token.clone());
    Ok(token)
}

#[tauri::command]
fn get_status(store: State<status::StatusStore>) -> Vec<status::ConnStatus> {
    store.all()
}

#[tauri::command]
fn test_rule(
    rule: rules::DetectionRule,
    from: String,
    subject: String,
    body: String,
) -> Result<Option<String>, String> {
    rules::test_rule(&rule, &from, &subject, &body).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn get_default_rules() -> Vec<rules::DetectionRule> {
    rules::default_rules()
}

#[tauri::command]
async fn restart_imap(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let accounts = config::load_accounts_with_passwords().map_err(|e| e.to_string())?;
    spawn_imap_tasks(app.clone(), &state, accounts).await.map_err(|e| e.to_string())
}

async fn spawn_imap_tasks(
    app: AppHandle,
    state: &AppState,
    accounts: Vec<AccountConfig>,
) -> Result<(), anyhow::Error> {
    {
        let mut tasks = state.tasks.lock().await;
        while let Some(task) = tasks.pop() {
            let _ = task.stop_tx.send(());
        }
    }

    let (tx, mut rx) = mpsc::channel::<String>(32);

    let event_handle = app.clone();
    async_runtime::spawn(async move {
        while let Some(payload) = rx.recv().await {
            tracing::info!("New auth code received: {payload}");
            let _ = event_handle.emit("auth-code", &payload);
            show_code_popup(&event_handle, payload);
        }
    });

    let mut tasks = state.tasks.lock().await;
    for account in accounts {
        if account.server.is_empty() || account.user.is_empty() {
            status::set(&app, &account.id, "error", "Server or username missing");
            continue;
        }
        if account.pass.is_empty() {
            status::set(&app, &account.id, "error", "No password stored – edit the account and enter it");
            continue;
        }
        let tx = tx.clone();
        let label = account.label.clone();
        let app_handle = app.clone();
        let check = state.check_now.clone();
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();

        async_runtime::spawn(async move {
            tokio::select! {
                _ = run_with_reconnect(app_handle, tx, account, check) => {}
                _ = stop_rx => {
                    tracing::info!("[{label}] IMAP task stopped");
                }
            }
        });

        tasks.push(ImapTask { stop_tx });
    }

    Ok(())
}

const RETRY_MIN: Duration = Duration::from_secs(5);
const RETRY_MAX: Duration = Duration::from_secs(5 * 60);
/// A connection that lived this long counts as healthy; the backoff restarts.
const HEALTHY_AFTER: Duration = Duration::from_secs(60);

/// Runs the IDLE loop forever, reconnecting with exponential backoff.
/// A manual check skips the remaining wait. Notifies only on the first
/// failure of a streak to avoid notification spam.
async fn run_with_reconnect(
    app: AppHandle,
    tx: mpsc::Sender<String>,
    account: AccountConfig,
    check: Arc<Notify>,
) {
    let label = account.label.clone();
    let mut delay = RETRY_MIN;
    let mut failures = 0u32;
    // Survives reconnects, so already handled mails don't pop up again.
    let last_uid = Arc::new(std::sync::atomic::AtomicU32::new(0));
    loop {
        let started = Instant::now();
        let Err(e) = imap::run_idle_loop(app.clone(), tx.clone(), account.clone(), check.clone(), last_uid.clone()).await else {
            continue;
        };
        if started.elapsed() > HEALTHY_AFTER {
            delay = RETRY_MIN;
            failures = 0;
        }
        failures += 1;
        tracing::error!("[{label}] IMAP task failed (attempt {failures}): {e:#}");
        if failures == 1 {
            let _ = app
                .notification()
                .builder()
                .title(format!("OTP-Scout – {label}"))
                .body(format!("Connection failed: {e:#}"))
                .show();
        }
        status::set(
            &app,
            &account.id,
            "error",
            format!("{e:#} – retrying in {}s", delay.as_secs()),
        );
        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            _ = check.notified() => tracing::info!("[{label}] Manual check – reconnecting now"),
        }
        delay = (delay * 2).min(RETRY_MAX);
    }
}

/// Last received code, so the popup can fetch it after (re)loading.
#[derive(Default)]
struct LastCode(std::sync::Mutex<Option<String>>);

#[tauri::command]
fn get_last_code(last: State<LastCode>) -> Option<String> {
    last.0.lock().unwrap().clone()
}

#[tauri::command]
fn copy_code(app: AppHandle, code: String) -> Result<(), String> {
    app.clipboard().write_text(code).map_err(|e| e.to_string())
}

/// Opens a detected login link in the default browser.
#[tauri::command]
fn open_link(url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("Only http(s) links can be opened".into());
    }
    std::process::Command::new("xdg-open")
        .arg(&url)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Failed to open browser: {e}"))
}

/// Deletes the mail a code/link came from.
#[tauri::command]
async fn delete_mail(account_id: String, uid: u32) -> Result<(), String> {
    let account = config::load_accounts_with_passwords()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|a| a.id == account_id)
        .ok_or("Account not found")?;
    imap::delete_message(&account, uid).await.map_err(|e| {
        tracing::error!("[{}] Delete failed: {e:#}", account.label);
        format!("{e:#}")
    })?;
    tracing::info!("[{}] Deleted message UID {uid}", account.label);
    Ok(())
}

#[tauri::command]
fn hide_code_popup(app: AppHandle) {
    if let Some(popup) = app.get_webview_window("code") {
        let _ = popup.hide();
    }
}

const POPUP_WIDTH: f64 = 400.0;

/// Centers the popup on the primary monitor's work area.
fn center_popup(popup: &tauri::WebviewWindow) {
    if let (Ok(Some(monitor)), Ok(size)) = (popup.primary_monitor(), popup.outer_size()) {
        let area = monitor.work_area();
        let x = area.position.x + (area.size.width as i32 - size.width as i32) / 2;
        let y = area.position.y + (area.size.height as i32 - size.height as i32) / 2;
        let _ = popup.set_position(tauri::PhysicalPosition::new(x, y));
    }
}

/// Adds an entry to the code popup and shows it.
fn show_code_popup(app: &AppHandle, payload: String) {
    *app.state::<LastCode>().0.lock().unwrap() = Some(payload.clone());
    let Some(popup) = app.get_webview_window("code") else { return };
    let _ = popup.emit_to("code", "show-code", &payload);
    let _ = popup.show();
    // Position after show(): GTK ignores moves of hidden windows.
    center_popup(&popup);
    let _ = popup.set_always_on_top(true);
    let _ = popup.set_focus();
}

/// The popup reports its content height when entries are added/removed.
#[tauri::command]
fn resize_code_popup(app: AppHandle, height: f64) {
    let Some(popup) = app.get_webview_window("code") else { return };
    let max = popup
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| m.work_area().size.height as f64 / m.scale_factor() * 0.85)
        .unwrap_or(900.0);
    let _ = popup.set_size(tauri::LogicalSize::new(POPUP_WIDTH, height.clamp(120.0, max)));
    center_popup(&popup);
}

fn spawn_check(app: AppHandle) {
    async_runtime::spawn(async move {
        if let Err(e) = trigger_check(app).await {
            tracing::error!("Manual check failed: {e:#}");
        }
    });
}

fn main() {
    tracing_subscriber::fmt::init();

    let accounts = config::load_accounts_with_passwords().unwrap_or_default();
    let config = Arc::new(Mutex::new(config::load_config().unwrap_or_default()));

    let app_state = AppState {
        config: config.clone(),
        tasks: Mutex::new(Vec::new()),
        check_now: Arc::new(Notify::new()),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(app_state)
        .manage(status::StatusStore::default())
        .manage(LastCode::default())
        .manage(api::ApiServer::default())
        .invoke_handler(tauri::generate_handler![
            get_accounts,
            add_account,
            update_account,
            remove_account,
            restart_imap,
            get_default_rules,
            get_status,
            test_rule,
            check_now,
            get_app_info,
            get_last_code,
            copy_code,
            open_link,
            delete_mail,
            hide_code_popup,
            resize_code_popup,
            get_api_settings,
            set_api_settings,
            regenerate_api_token,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            async_runtime::spawn(api::restart(handle.clone()));

            let handle_clone = handle.clone();
            let accounts_clone = accounts;
            async_runtime::spawn(async move {
                let state = handle_clone.state::<AppState>();
                if let Err(e) = spawn_imap_tasks(handle_clone.clone(), &state, accounts_clone).await {
                    tracing::error!("Failed to start IMAP tasks: {e}");
                }
            });

            let settings = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("index.html".into()))
                .title("OTP-Scout Settings")
                .inner_size(820.0, 760.0)
                .resizable(true)
                .visible(false)
                .build()?;

            let settings_clone = settings.clone();
            settings.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = settings_clone.hide();
                }
            });

            let about = WebviewWindowBuilder::new(app, "about", WebviewUrl::App("about.html".into()))
                .title("About OTP-Scout")
                .inner_size(360.0, 340.0)
                .resizable(false)
                .visible(false)
                .build()?;
            let about_clone = about.clone();
            about.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = about_clone.hide();
                }
            });

            WebviewWindowBuilder::new(app, "code", WebviewUrl::App("code.html".into()))
                .title("OTP-Scout Code")
                .inner_size(POPUP_WIDTH, 250.0)
                .resizable(false)
                .decorations(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .visible(false)
                .build()?;

            let check_item = MenuItemBuilder::with_id("check_now", "Check now").build(app)?;
            let settings_item = MenuItemBuilder::with_id("open_settings", "Settings").build(app)?;
            let about_item = MenuItemBuilder::with_id("about", "About").build(app)?;
            let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
            let menu = MenuBuilder::new(app)
                .item(&check_item)
                .separator()
                .item(&settings_item)
                .item(&about_item)
                .separator()
                .item(&quit)
                .build()?;

            let icon = Image::from_bytes(include_bytes!("../icons/icon.png"))
                .expect("failed to load tray icon");

            let settings_handle = settings.clone();
            let _tray = TrayIconBuilder::new()
                .icon(icon)
                .menu(&menu)
                .tooltip("OTP-Scout")
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    "quit" => app.exit(0),
                    "check_now" => spawn_check(app.clone()),
                    "open_settings" => {
                        let _ = settings_handle.show();
                        let _ = settings_handle.set_focus();
                    }
                    "about" => {
                        let _ = about.show();
                        let _ = about.set_focus();
                    }
                    _ => {}
                })
                .on_tray_icon_event(move |tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        // Not emitted on Linux (AppIndicator) – there "Check now" is in the menu.
                        spawn_check(tray.app_handle().clone());
                    }
                })
                .build(app)?;

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            if let RunEvent::ExitRequested { .. } = event {}
        });
}