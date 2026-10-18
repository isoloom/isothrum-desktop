fn main() {
    // The app's own commands, so capabilities can grant them (to the editor's origin only).
    let commands = &[
        "desktop_info",
        "environment_up",
        "environment_down",
        "environment_status",
        "environment_live",
        "capture_start",
        "capture_stop",
        "machine_console",
        "machine_display",
        "machine_action",
        "link_list",
        "link_action",
        "snapshot_list",
        "snapshot_take",
        "snapshot_use",
        "snapshot_delete",
        "update_check",
        "update_install",
    ];
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(commands)),
    )
    .expect("tauri build");
}
