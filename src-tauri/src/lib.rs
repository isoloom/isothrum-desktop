//! Isothrum desktop: the editor from isothrum.com in a window, plus what a browser can't do:
//! running a lab on this machine and capturing its traffic. The editor finds these commands on
//! `window.__TAURI__` and shows Run, Stop and Capture only here.

mod capabilities;
mod captures;
mod console;
mod lab_files;
mod lab_name;
mod runner;
mod terminal;
mod tools;
mod vms;

use std::path::{Path, PathBuf};

use serde::Serialize;

use captures::Captures;
use lab_name::checked;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};

// The window shows the editor bundled with the app (`apps/desktop/editor`, built from the website
// by `npm run editor`): it works offline. `ISOTHRUM_URL` points it at a site instead (isothrum.com
// for accounts and saved projects, or a local build while developing).

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct DesktopInfo {
    version: &'static str,
    /// Whether `docker` answers (installed, running, and this user may use it).
    docker: bool,
    /// Whether Vagrant is installed (labs with VMs run with it).
    vagrant: bool,
    wireshark: bool,
    /// Wireshark can show captures live (its `dumpcap` is usable by this account).
    live_capture: bool,
    labs: String,
}

#[tauri::command]
async fn desktop_info(app: AppHandle) -> Result<DesktopInfo, String> {
    let labs = data_dir(&app)?.join("labs").display().to_string();
    let docker = tauri::async_runtime::spawn_blocking(|| {
        std::process::Command::new("docker")
            .args(["info", "--format", "{{.ServerVersion}}"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
    .await
    .unwrap_or(false);
    Ok(DesktopInfo {
        version: env!("CARGO_PKG_VERSION"),
        docker,
        vagrant: tools::has("vagrant"),
        wireshark: tools::has("wireshark"),
        live_capture: runner::live_capture_possible(),
        labs,
    })
}

/// One of the lab's own files (a provisioning script, an appliance's configuration).
#[derive(serde::Deserialize)]
struct LabFile {
    path: String,
    contents: String,
}

/// Writes the lab (its isoloom.yml, its own files and the generated ones) and starts it.
#[tauri::command]
async fn environment_up(
    app: AppHandle,
    name: String,
    yaml: String,
    files: Option<Vec<LabFile>>,
) -> Result<String, String> {
    let dir = runner::lab_dir(&data_dir(&app)?, checked(&name)?);
    let own: Vec<(String, String)> = files
        .unwrap_or_default()
        .into_iter()
        .map(|f| (f.path, f.contents))
        .collect();
    tauri::async_runtime::spawn_blocking(move || {
        runner::prepare(&dir, &yaml, &own)?;
        runner::up(&dir)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn environment_down(
    app: AppHandle,
    captures: State<'_, Captures>,
    name: String,
) -> Result<String, String> {
    let name = checked(&name)?.to_string();
    // Stopping the lab ends its captures too.
    captures.stop_lab(&name);
    let dir = runner::lab_dir(&data_dir(&app)?, &name);
    tauri::async_runtime::spawn_blocking(move || runner::down(&dir))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn environment_status(app: AppHandle, name: String) -> Result<String, String> {
    let dir = runner::lab_dir(&data_dir(&app)?, checked(&name)?);
    if !runner::prepared(&dir) {
        return Ok(String::new());
    }
    tauri::async_runtime::spawn_blocking(move || runner::status(&dir))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct CaptureStarted {
    /// Where the packets are written when Wireshark isn't installed; `None`: Wireshark is open.
    file: Option<String>,
}

/// Where a capture without Wireshark goes: `<data>/captures/<lab>-<network>-<seconds>.pcap`.
fn capture_file(data: &Path, lab: &str, network: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    data.join("captures")
        .join(format!("{lab}-{network}-{stamp}.pcap"))
}

/// Captures a LAN's or a cable's traffic (`network` is its name in isoloom.yml).
#[tauri::command]
async fn capture_start(
    app: AppHandle,
    captures: State<'_, Captures>,
    name: String,
    network: String,
) -> Result<CaptureStarted, String> {
    let name = checked(&name)?.to_string();
    let data = data_dir(&app)?;
    let dir = runner::lab_dir(&data, &name);
    let file = capture_file(&data, &name, &network);
    let net = network.clone();
    let capture = tauri::async_runtime::spawn_blocking(move || {
        let plan = runner::plan_capture(&dir, &runner::lab_spec(&dir)?, &net)?;
        runner::start_capture(&plan, &file)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    let file = capture.file.as_ref().map(|f| f.display().to_string());
    captures.add(&name, &network, capture);
    Ok(CaptureStarted { file })
}

#[tauri::command]
fn capture_stop(
    captures: State<'_, Captures>,
    name: String,
    network: String,
) -> Result<(), String> {
    captures.stop(checked(&name)?, &network);
    Ok(())
}

/// Opens a terminal on one machine of the running lab (a shell, or SSH to a network appliance).
#[tauri::command]
async fn machine_console(app: AppHandle, name: String, machine: String) -> Result<(), String> {
    let name = checked(&name)?.to_string();
    let dir = runner::lab_dir(&data_dir(&app)?, &name);
    tauri::async_runtime::spawn_blocking(move || {
        let spec = runner::lab_spec(&dir)?;
        let cmd = runner::console_command(&dir, &spec, &machine)?;
        terminal::open_terminal(&format!("{machine} ({name})"), &cmd)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

/// Opens one machine's screen (GNS3's VNC console): a container's VNC server, or a VM's display.
/// Returns what was opened (a `vnc://` address, or the VM's window).
#[tauri::command]
async fn machine_display(app: AppHandle, name: String, machine: String) -> Result<String, String> {
    let dir = runner::lab_dir(&data_dir(&app)?, checked(&name)?);
    tauri::async_runtime::spawn_blocking(move || {
        let spec = runner::lab_spec(&dir)?;
        runner::open_display(&dir, &spec, &machine)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

/// Starts, stops or restarts one machine of the running lab.
#[tauri::command]
async fn machine_action(
    app: AppHandle,
    name: String,
    machine: String,
    action: String,
) -> Result<String, String> {
    let dir = runner::lab_dir(&data_dir(&app)?, checked(&name)?);
    tauri::async_runtime::spawn_blocking(move || {
        let spec = runner::lab_spec(&dir)?;
        runner::machine_action(&dir, &spec, &machine, &action)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

/// The environment now: each machine's state, and each machine's port on each network, up or down
/// (read from the machines). Empty when the environment hasn't been started.
#[tauri::command]
async fn environment_live(
    app: AppHandle,
    name: String,
) -> Result<Option<runner::EnvironmentLive>, String> {
    let dir = runner::lab_dir(&data_dir(&app)?, checked(&name)?);
    if !runner::prepared(&dir) {
        return Ok(None);
    }
    tauri::async_runtime::spawn_blocking(move || {
        let spec = runner::lab_spec(&dir)?;
        runner::environment_live(&dir, &spec).map(Some)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

/// The lab's suspended links (a machine's port on a network, cut).
#[tauri::command]
fn link_list(app: AppHandle, name: String) -> Result<Vec<runner::Link>, String> {
    let dir = runner::lab_dir(&data_dir(&app)?, checked(&name)?);
    let links = runner::suspended_links(&dir);
    Ok(match runner::lab_spec(&dir) {
        Ok(spec) => runner::editor_links(&spec, links),
        Err(_) => links,
    })
}

/// Suspends or resumes a link of the running lab: the ports of `machines` on `network` (a
/// cable's two ends, or one machine's port on a LAN), or of every machine on it.
#[tauri::command]
async fn link_action(
    app: AppHandle,
    name: String,
    network: String,
    machines: Option<Vec<String>>,
    action: String,
) -> Result<Vec<runner::Link>, String> {
    let dir = runner::lab_dir(&data_dir(&app)?, checked(&name)?);
    tauri::async_runtime::spawn_blocking(move || {
        let spec = runner::lab_spec(&dir)?;
        runner::link_action(&dir, &spec, &network, machines, &action)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

/// The lab's snapshots.
#[tauri::command]
fn snapshot_list(app: AppHandle, name: String) -> Result<Vec<runner::Snapshot>, String> {
    Ok(runner::snapshots(&runner::lab_dir(
        &data_dir(&app)?,
        checked(&name)?,
    )))
}

/// Saves one machine of the running lab.
#[tauri::command]
async fn snapshot_take(
    app: AppHandle,
    name: String,
    machine: String,
    label: String,
) -> Result<runner::Snapshot, String> {
    let name = checked(&name)?.to_string();
    let dir = runner::lab_dir(&data_dir(&app)?, &name);
    tauri::async_runtime::spawn_blocking(move || {
        runner::take_snapshot(&dir, &name, &machine, &label)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

/// Runs a machine from one of its snapshots (`id`), or from its own image again (no `id`).
#[tauri::command]
async fn snapshot_use(
    app: AppHandle,
    name: String,
    machine: String,
    id: Option<String>,
) -> Result<String, String> {
    let dir = runner::lab_dir(&data_dir(&app)?, checked(&name)?);
    tauri::async_runtime::spawn_blocking(move || {
        runner::use_snapshot(&dir, &machine, id.as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn snapshot_delete(app: AppHandle, name: String, id: String) -> Result<(), String> {
    let dir = runner::lab_dir(&data_dir(&app)?, checked(&name)?);
    tauri::async_runtime::spawn_blocking(move || runner::delete_snapshot(&dir, &id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// A newer release, if there is one (the updater checks GitHub's latest release, signed with
/// the app's key).
#[derive(Serialize)]
struct UpdateInfo {
    version: String,
    notes: Option<String>,
}

#[tauri::command]
async fn update_check(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    use tauri_plugin_updater::UpdaterExt;
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;
    Ok(update.map(|u| UpdateInfo {
        version: u.version,
        notes: u.body,
    }))
}

/// Downloads and installs the newer release, then restarts the app on it.
#[tauri::command]
async fn update_install(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;
    let Some(update) = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    app.restart();
}

pub fn run() {
    tools::complete_path();
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(Captures::<runner::Capture>::default())
        .invoke_handler(tauri::generate_handler![
            desktop_info,
            environment_up,
            environment_down,
            environment_status,
            environment_live,
            capture_start,
            capture_stop,
            machine_console,
            machine_display,
            machine_action,
            link_list,
            link_action,
            snapshot_list,
            snapshot_take,
            snapshot_use,
            snapshot_delete,
            update_check,
            update_install
        ])
        .setup(|app| {
            capabilities::allow_local_site(app)?;
            open_main_window(app)
        })
        .build(tauri::generate_context!())
        .expect("Isothrum desktop failed to start")
        .run(on_event);
}

/// The window: the bundled editor, or the site `ISOTHRUM_URL` names.
fn open_main_window(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let page = match std::env::var("ISOTHRUM_URL") {
        Ok(url) => WebviewUrl::External(url.parse().map_err(|e| format!("ISOTHRUM_URL: {e}"))?),
        Err(_) => WebviewUrl::App("index.html".into()),
    };
    WebviewWindowBuilder::new(app, "main", page)
        .title("Isothrum")
        .inner_size(1440.0, 900.0)
        .min_inner_size(900.0, 600.0)
        .build()?;
    Ok(())
}

/// Leaving the app ends every capture it started.
fn on_event(app: &AppHandle, event: tauri::RunEvent) {
    if let tauri::RunEvent::Exit = event
        && let Some(c) = app.try_state::<Captures>()
    {
        c.stop_all();
    }
}
