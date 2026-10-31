// App commands get `allow-<command>` permissions; capabilities/main.json grants them by name.
const COMMANDS: &[&str] = &[
    "app_info",
    "list_meetings",
    "search",
    "get_meeting",
    "rename_meeting",
    "start_recording",
    "stop_recording",
    "pause_recording",
    "resume_recording",
    "live_snapshot",
    "get_settings",
    "set_settings",
    "list_input_devices",
    "permissions",
    "open_privacy_settings",
    "smoke_report",
    "edit_segment",
    "revert_segment",
    "restore_segment",
    "list_glossary",
    "save_glossary_entry",
    "delete_glossary_entry",
    "export_meeting",
    "delete_meeting",
    "storage_usage",
];

fn main() {
    let attributes = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS));
    tauri_build::try_build(attributes).expect("tauri-build");
}
