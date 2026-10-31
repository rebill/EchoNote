//! Audio capture sources.

#[cfg(target_os = "macos")]
pub mod macos_tap;

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("{what} failed with OSStatus {status} ({})", fourcc(*status))]
    Os { what: &'static str, status: i32 },
}

/// Render an OSStatus as its four-character code when printable.
fn fourcc(status: i32) -> String {
    let bytes = status.to_be_bytes();
    if bytes
        .iter()
        .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
    {
        format!("'{}'", String::from_utf8_lossy(&bytes))
    } else {
        "-".into()
    }
}
