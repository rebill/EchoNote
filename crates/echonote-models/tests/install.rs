//! Installs against a local HTTP server that can drop connections, ignore `Range`, serve
//! wrong bytes or nothing, and host a release archive.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use echonote_models::{Archive, Cancel, File, Installer, Model, ModelError, Progress, Role, Stage};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[derive(Default)]
struct Plan {
    files: HashMap<String, Vec<u8>>,
    /// The first answer for these paths stops halfway.
    cut_once: Mutex<HashSet<String>>,
    ignore_range: bool,
    /// Path and `Range` start of every request.
    log: Mutex<Vec<(String, Option<u64>)>>,
}

impl Plan {
    fn requests(&self) -> Vec<(String, Option<u64>)> {
        self.log.lock().unwrap().clone()
    }
}

async fn serve(plan: Plan) -> (Arc<Plan>, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let plan = Arc::new(plan);
    let shared = plan.clone();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(respond(stream, shared.clone()));
        }
    });
    (plan, base)
}

async fn respond(mut stream: TcpStream, plan: Arc<Plan>) {
    let mut head = Vec::new();
    let mut buffer = [0u8; 1024];
    while !head.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(read) => head.extend_from_slice(&buffer[..read]),
        }
    }
    let head = String::from_utf8_lossy(&head).into_owned();
    let path = head.split_whitespace().nth(1).unwrap_or("/").to_owned();
    let range = head
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(name, _)| name.eq_ignore_ascii_case("range"))
        })
        .and_then(|(_, value)| {
            value
                .trim()
                .strip_prefix("bytes=")?
                .trim_end_matches('-')
                .parse::<u64>()
                .ok()
        });
    plan.log.lock().unwrap().push((path.clone(), range));
    let Some(data) = plan.files.get(&path) else {
        let _ = stream
            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await;
        return;
    };
    let cut = plan.cut_once.lock().unwrap().remove(&path);
    let (status, start) = match range {
        Some(start) if !plan.ignore_range && start < data.len() as u64 => {
            ("206 Partial Content", start as usize)
        }
        Some(_) if !plan.ignore_range => {
            let _ = stream
                .write_all(b"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await;
            return;
        }
        _ => ("200 OK", 0),
    };
    let body = &data[start..];
    let mut reply = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    if start > 0 {
        reply += &format!(
            "Content-Range: bytes {start}-{}/{}\r\n",
            data.len() - 1,
            data.len()
        );
    }
    reply += "\r\n";
    if stream.write_all(reply.as_bytes()).await.is_err() {
        return;
    }
    let sent = if cut { &body[..body.len() / 2] } else { body };
    for chunk in sent.chunks(16 * 1024) {
        if stream.write_all(chunk).await.is_err() {
            return;
        }
    }
    let _ = stream.flush().await;
}

fn bytes(seed: u8, len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| ((i * 31 + seed as usize * 7) % 251) as u8)
        .collect()
}

fn sha(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

fn model(files: &[(&str, &[u8])], mirrors: Vec<String>, archive: Option<Archive>) -> Model {
    Model {
        id: "test".into(),
        role: Role::Accurate,
        dir: "test-model".into(),
        kind: Some("qwen3-asr".into()),
        license: "test".into(),
        memory_mb: 0,
        files: files
            .iter()
            .map(|(path, data)| File {
                path: (*path).into(),
                size: data.len() as u64,
                sha256: sha(data),
            })
            .collect(),
        mirrors,
        archive,
    }
}

fn tar_bz2(prefix: &str, files: &[(&str, &[u8])]) -> Vec<u8> {
    let encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
    let mut tar = tar::Builder::new(encoder);
    for (path, data) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, format!("{prefix}/{path}"), *data)
            .unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap()
}

fn installer() -> Installer {
    Installer::new()
        .unwrap()
        .with_retry_delay(Duration::from_millis(10))
}

fn run<T>(future: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}

async fn install(model: &Model, target: &Path) -> Result<Vec<Progress>, ModelError> {
    let mut seen = Vec::new();
    installer()
        .install(model, target, &Cancel::new(), &mut |progress| {
            seen.push(progress)
        })
        .await?;
    Ok(seen)
}

fn assert_installed(target: &Path, files: &[(&str, &[u8])]) {
    for (path, data) in files {
        assert_eq!(std::fs::read(target.join(path)).unwrap(), *data, "{path}");
    }
    assert!(!echonote_models::staging_dir(target).exists());
}

#[test]
fn installs_from_the_first_mirror_and_skips_a_complete_model() {
    let (a, b) = (bytes(1, 70_000), bytes(2, 3_000));
    let files: [(&str, &[u8]); 2] = [("model.onnx", &a), ("tokenizer/vocab.json", &b)];
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("test-model");
    run(async {
        let served = [
            ("/m/model.onnx".to_owned(), a.clone()),
            ("/m/tokenizer/vocab.json".to_owned(), b.clone()),
        ];
        let (plan, base) = serve(Plan {
            files: served.into(),
            ..Plan::default()
        })
        .await;
        let model = model(&files, vec![format!("{base}/m/")], None);
        let seen = install(&model, &target).await.unwrap();
        assert_installed(&target, &files);
        assert_eq!(
            seen.last().unwrap(),
            &Progress {
                stage: Stage::Downloading,
                done: 73_000,
                total: 73_000
            }
        );
        assert!(seen.windows(2).all(|pair| pair[0].done <= pair[1].done));
        assert_eq!(plan.requests().len(), 2);
        install(&model, &target).await.unwrap();
        assert_eq!(
            plan.requests().len(),
            2,
            "a complete model is not fetched again"
        );
    });
}

#[test]
fn a_same_size_damaged_copy_is_repaired() {
    let (a, b) = (bytes(11, 40_000), bytes(12, 2_000));
    let files: [(&str, &[u8]); 2] = [("model.onnx", &a), ("tokenizer/vocab.json", &b)];
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("test-model");
    run(async {
        let served = [
            ("/m/model.onnx".to_owned(), a.clone()),
            ("/m/tokenizer/vocab.json".to_owned(), b.clone()),
        ];
        let (plan, base) = serve(Plan {
            files: served.into(),
            ..Plan::default()
        })
        .await;
        let model = model(&files, vec![format!("{base}/m")], None);
        install(&model, &target).await.unwrap();
        let mut damaged = b.clone();
        damaged[7] ^= 0xff;
        std::fs::write(target.join("tokenizer/vocab.json"), &damaged).unwrap();
        assert!(model.is_complete(&target), "the sizes still match");

        install(&model, &target).await.unwrap();
        assert_installed(&target, &files);
        let requests = plan.requests();
        assert_eq!(requests.len(), 3, "{requests:?}");
        assert_eq!(
            requests[2].0, "/m/tokenizer/vocab.json",
            "only the bad file"
        );
    });
}

#[test]
fn a_dropped_connection_resumes_where_it_stopped() {
    let data = bytes(3, 200_000);
    let files: [(&str, &[u8]); 1] = [("model.onnx", &data)];
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("test-model");
    run(async {
        let plan = Plan {
            files: [("/m/model.onnx".to_owned(), data.clone())].into(),
            cut_once: Mutex::new(["/m/model.onnx".to_owned()].into()),
            ..Plan::default()
        };
        let (plan, base) = serve(plan).await;
        install(&model(&files, vec![format!("{base}/m")], None), &target)
            .await
            .unwrap();
        assert_installed(&target, &files);
        let requests = plan.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].1, None);
        assert_eq!(requests[1].1, Some(100_000));
    });
}

#[test]
fn a_cancelled_download_keeps_its_bytes_for_next_time() {
    let data = bytes(4, 2_000_000);
    let files: [(&str, &[u8]); 1] = [("model.onnx", &data)];
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("test-model");
    run(async {
        let (plan, base) = serve(Plan {
            files: [("/m/model.onnx".to_owned(), data.clone())].into(),
            ..Plan::default()
        })
        .await;
        let model = model(&files, vec![format!("{base}/m")], None);
        let cancel = Cancel::new();
        let stop = cancel.clone();
        let result = installer()
            .install(&model, &target, &cancel, &mut |progress| {
                if progress.done > 0 {
                    stop.cancel();
                }
            })
            .await;
        assert!(matches!(result, Err(ModelError::Cancelled)), "{result:?}");
        assert!(!target.exists());
        let kept = echonote_models::staged_bytes(&model, &target);
        assert!(kept > 0 && kept < data.len() as u64, "{kept}");

        install(&model, &target).await.unwrap();
        assert_installed(&target, &files);
        assert_eq!(plan.requests().last().unwrap().1, Some(kept));
    });
}

#[test]
fn a_cancel_before_the_start_downloads_nothing() {
    let data = bytes(5, 1_000);
    let files: [(&str, &[u8]); 1] = [("model.onnx", &data)];
    let dir = tempfile::tempdir().unwrap();
    run(async {
        let (plan, base) = serve(Plan {
            files: [("/m/model.onnx".to_owned(), data.clone())].into(),
            ..Plan::default()
        })
        .await;
        let cancel = Cancel::new();
        cancel.cancel();
        let model = model(&files, vec![format!("{base}/m")], None);
        let result = installer()
            .install(&model, &dir.path().join("test-model"), &cancel, &mut |_| {})
            .await;
        assert!(matches!(result, Err(ModelError::Cancelled)), "{result:?}");
        assert!(plan.requests().is_empty());
    });
}

#[test]
fn wrong_bytes_move_on_to_the_next_mirror() {
    let data = bytes(6, 50_000);
    let mut wrong = data.clone();
    wrong[123] ^= 0xff;
    let files: [(&str, &[u8]); 1] = [("model.onnx", &data)];
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("test-model");
    run(async {
        let served = [
            ("/bad/model.onnx".to_owned(), wrong),
            ("/good/model.onnx".to_owned(), data.clone()),
        ];
        let (plan, base) = serve(Plan {
            files: served.into(),
            ..Plan::default()
        })
        .await;
        let mirrors = vec![
            format!("{base}/missing"),
            format!("{base}/bad"),
            format!("{base}/good"),
        ];
        install(&model(&files, mirrors, None), &target)
            .await
            .unwrap();
        assert_installed(&target, &files);
        let paths: Vec<String> = plan.requests().into_iter().map(|(path, _)| path).collect();
        // A 404 and a clean checksum failure are not retried on the same mirror.
        assert_eq!(
            paths,
            ["/missing/model.onnx", "/bad/model.onnx", "/good/model.onnx"]
        );
    });
}

#[test]
fn a_bad_partial_file_is_downloaded_again_from_the_start() {
    let data = bytes(7, 40_000);
    let files: [(&str, &[u8]); 1] = [("model.onnx", &data)];
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("test-model");
    let staging = echonote_models::staging_dir(&target);
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::write(staging.join("model.onnx.part"), vec![0u8; 10_000]).unwrap();
    run(async {
        let (plan, base) = serve(Plan {
            files: [("/m/model.onnx".to_owned(), data.clone())].into(),
            ..Plan::default()
        })
        .await;
        install(&model(&files, vec![format!("{base}/m")], None), &target)
            .await
            .unwrap();
        assert_installed(&target, &files);
        let ranges: Vec<Option<u64>> = plan
            .requests()
            .into_iter()
            .map(|(_, range)| range)
            .collect();
        assert_eq!(ranges, [Some(10_000), None]);
    });
}

#[test]
fn a_server_that_ignores_range_sends_the_whole_file() {
    let data = bytes(8, 40_000);
    let files: [(&str, &[u8]); 1] = [("model.onnx", &data)];
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("test-model");
    let staging = echonote_models::staging_dir(&target);
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::write(staging.join("model.onnx.part"), &data[..15_000]).unwrap();
    run(async {
        let plan = Plan {
            files: [("/m/model.onnx".to_owned(), data.clone())].into(),
            ignore_range: true,
            ..Plan::default()
        };
        let (plan, base) = serve(plan).await;
        install(&model(&files, vec![format!("{base}/m")], None), &target)
            .await
            .unwrap();
        assert_installed(&target, &files);
        assert_eq!(plan.requests().len(), 1);
    });
}

const PREFIX: &str = "sherpa-onnx-test";

fn archive_for(base: &str, data: &[u8]) -> Archive {
    Archive {
        url: format!("{base}/release.tar.bz2"),
        size: data.len() as u64,
        sha256: sha(data),
        prefix: PREFIX.into(),
    }
}

#[test]
fn the_release_archive_supplies_what_the_mirrors_lack() {
    let (a, b) = (bytes(9, 60_000), bytes(10, 5_000));
    let files: [(&str, &[u8]); 2] = [("model.onnx", &a), ("tokenizer/vocab.json", &b)];
    let packed = tar_bz2(
        PREFIX,
        &[
            ("README.md", b"sample"),
            ("model.onnx", &a),
            ("tokenizer/vocab.json", &b),
        ],
    );
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("test-model");
    run(async {
        // The mirror has the model but not the tokenizer.
        let served = [
            ("/m/model.onnx".to_owned(), a.clone()),
            ("/release.tar.bz2".to_owned(), packed.clone()),
        ];
        let (plan, base) = serve(Plan {
            files: served.into(),
            ..Plan::default()
        })
        .await;
        let model = model(
            &files,
            vec![format!("{base}/m")],
            Some(archive_for(&base, &packed)),
        );
        let seen = install(&model, &target).await.unwrap();
        assert_installed(&target, &files);
        assert!(!target.join(".archive").exists() && !target.join("README.md").exists());
        assert!(
            seen.iter()
                .any(|progress| progress.stage == Stage::Extracting)
        );
        assert_eq!(seen.last().unwrap().done, 65_000);
        let paths: Vec<String> = plan.requests().into_iter().map(|(path, _)| path).collect();
        assert_eq!(
            paths,
            [
                "/m/model.onnx",
                "/m/tokenizer/vocab.json",
                "/release.tar.bz2"
            ]
        );
    });
}

#[test]
fn an_archive_without_a_file_fails_and_says_which() {
    let (a, b) = (bytes(11, 1_000), bytes(12, 1_000));
    let files: [(&str, &[u8]); 2] = [("model.onnx", &a), ("tokens.txt", &b)];
    let packed = tar_bz2(PREFIX, &[("model.onnx", &a)]);
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("test-model");
    run(async {
        let (_, base) = serve(Plan {
            files: [("/release.tar.bz2".to_owned(), packed.clone())].into(),
            ..Plan::default()
        })
        .await;
        let model = model(
            &files,
            vec![format!("{base}/m")],
            Some(archive_for(&base, &packed)),
        );
        let result = install(&model, &target).await;
        assert!(
            matches!(&result, Err(ModelError::MissingInArchive(path)) if path == "tokens.txt"),
            "{result:?}"
        );
        assert!(!target.exists());
    });
}

#[test]
fn a_full_disk_is_reported_before_downloading() {
    let dir = tempfile::tempdir().unwrap();
    run(async {
        let (plan, base) = serve(Plan::default()).await;
        let mut model = model(&[("model.onnx", b"x")], vec![format!("{base}/m")], None);
        model.files[0].size = u64::MAX / 4;
        let result = install(&model, &dir.path().join("test-model")).await;
        assert!(
            matches!(result, Err(ModelError::DiskFull { .. })),
            "{result:?}"
        );
        assert_eq!(result.unwrap_err().code(), "model-disk-full");
        assert!(plan.requests().is_empty());
    });
}

#[test]
fn an_incomplete_model_directory_is_replaced_and_delete_removes_everything() {
    let data = bytes(13, 8_000);
    let files: [(&str, &[u8]); 1] = [("model.onnx", &data)];
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("test-model");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("model.onnx"), b"truncated").unwrap();
    run(async {
        let (_, base) = serve(Plan {
            files: [("/m/model.onnx".to_owned(), data.clone())].into(),
            ..Plan::default()
        })
        .await;
        install(&model(&files, vec![format!("{base}/m")], None), &target)
            .await
            .unwrap();
    });
    assert_installed(&target, &files);
    std::fs::create_dir_all(echonote_models::staging_dir(&target)).unwrap();
    echonote_models::delete(&target).unwrap();
    assert!(!target.exists() && !echonote_models::staging_dir(&target).exists());
    echonote_models::delete(&target).unwrap();
}

#[test]
fn plain_http_is_refused_outside_this_machine() {
    let data = bytes(14, 100);
    let files: [(&str, &[u8]); 1] = [("model.onnx", &data)];
    let dir = tempfile::tempdir().unwrap();
    run(async {
        let model = model(&files, vec!["http://example.com/m".into()], None);
        let result = install(&model, &dir.path().join("test-model")).await;
        assert!(matches!(result, Err(ModelError::Network(_))), "{result:?}");
    });
}
