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
    "get_ai",
    "save_provider",
    "delete_provider",
    "set_ai_options",
    "consent_provider",
    "test_provider",
    "list_models",
    "save_template",
    "delete_template",
    "summarize",
    "ask",
    "correct_meeting",
    "keep_corrections",
    "revert_correction",
    "clear_qa",
    "cancel_job",
    "running_jobs",
    "get_highlights",
    "asr_models",
    "download_model",
    "cancel_download",
    "delete_model",
    "retranscribe",
    "revert_retranscription",
];

fn main() {
    // Replaces Tauri's default Windows manifest; the file keeps what that one has.
    let windows = tauri_build::WindowsAttributes::new()
        .app_manifest(include_str!("windows-app-manifest.xml"));
    let attributes = tauri_build::Attributes::new()
        .windows_attributes(windows)
        .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS));
    tauri_build::try_build(attributes).expect("tauri-build");
}
