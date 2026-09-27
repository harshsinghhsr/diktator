//! The only network code in Diktator. Downloads a catalog model with HTTP
//! Range resume, verifies its SHA-256, extracts archives, and installs by
//! renaming a fully prepared staging directory into place.

use crate::catalog::{self, ModelId};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Downloading,
    Verifying,
    Extracting,
    Done,
    Error,
    Cancelled,
}

#[derive(Clone, Debug, Serialize)]
pub struct Progress {
    pub id: ModelId,
    pub downloaded: u64,
    pub total: u64,
    pub status: Status,
    pub error: Option<String>,
}

impl Progress {
    pub fn new(id: ModelId, downloaded: u64, total: u64, status: Status) -> Self {
        Self { id, downloaded, total, status, error: None }
    }
}

#[derive(Debug)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("download cancelled")
    }
}

impl std::error::Error for Cancelled {}

const CHUNK: usize = 256 * 1024;

fn client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent(concat!("Diktator/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(20))
        // The blocking client's default 30 s *total* timeout would kill every
        // large model download.
        .timeout(None::<Duration>)
        .build()?)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Downloads `url` into `dest`, appending to an existing partial `dest` via a
/// Range request. `on_bytes(downloaded, total)` is called per chunk. On cancel
/// returns `Cancelled` and keeps the partial file for the next attempt.
pub fn fetch(url: &str, dest: &Path, cancel: &AtomicBool, on_bytes: &mut dyn FnMut(u64, u64)) -> Result<()> {
    if let Some(dir) = dest.parent() {
        fs::create_dir_all(dir)?;
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(Cancelled.into());
    }
    let have = fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
    let mut req = client()?.get(url);
    if have > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={have}-"));
    }
    let mut resp = req.send().with_context(|| format!("could not reach {url}"))?;
    let status = resp.status();
    let (mut file, mut done, total) = if status == reqwest::StatusCode::PARTIAL_CONTENT {
        let rest = resp.content_length().unwrap_or(0);
        (OpenOptions::new().append(true).open(dest)?, have, have + rest)
    } else if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE && have > 0 {
        on_bytes(have, have); // already complete from an earlier attempt
        return Ok(());
    } else if status.is_success() {
        (File::create(dest)?, 0, resp.content_length().unwrap_or(0))
    } else {
        bail!("Download failed: the server answered HTTP {status}.");
    };
    let mut buf = vec![0u8; CHUNK];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(Cancelled.into());
        }
        let n = resp.read(&mut buf).context("Download interrupted. It will resume where it stopped.")?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        done += n as u64;
        on_bytes(done, total.max(done));
    }
    file.flush()?;
    if total > 0 && done < total {
        bail!("Download interrupted. It will resume where it stopped.");
    }
    Ok(())
}

/// Checks SHA-256; deletes the file on mismatch so the next attempt starts clean.
pub fn verify(path: &Path, sha256_hex: &str) -> Result<()> {
    let mut f = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let got = hex(hasher.finalize().as_slice());
    if !got.eq_ignore_ascii_case(sha256_hex) {
        let _ = fs::remove_file(path);
        bail!("Checksum mismatch: the download was corrupted. Try again.");
    }
    Ok(())
}

/// Turns a verified payload into an installed model directory. Nothing is
/// visible at the final path until every expected file is present.
pub fn install_payload(root: &Path, id: ModelId, payload: &Path) -> Result<()> {
    let info = id.info();
    let staging = root.join(format!(".staging-{}", id.slug()));
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging)?;
    let prepared = (|| -> Result<()> {
        if info.archive {
            let decoder = bzip2::read::BzDecoder::new(File::open(payload)?);
            tar::Archive::new(decoder).unpack(&staging).context("Could not extract the model archive.")?;
        } else {
            fs::copy(payload, staging.join(info.files[0]))?;
        }
        for f in info.files {
            if !staging.join(f).is_file() {
                bail!("The downloaded model is missing {f}.");
            }
        }
        Ok(())
    })();
    if let Err(e) = prepared {
        let _ = fs::remove_dir_all(&staging);
        return Err(e);
    }
    let final_dir = catalog::model_dir(root, id);
    let _ = fs::remove_dir_all(&final_dir);
    fs::rename(&staging, &final_dir)?;
    let _ = fs::remove_file(payload);
    Ok(())
}

fn part_path(root: &Path, id: ModelId) -> PathBuf {
    root.join(".downloads").join(format!("{}.part", id.slug()))
}

/// Full install: download (resumable) → verify → extract → move into place.
pub fn install(root: &Path, id: ModelId, cancel: &AtomicBool, on_progress: &mut dyn FnMut(Progress)) -> Result<()> {
    let info = id.info();
    let part = part_path(root, id);
    fetch(info.url, &part, cancel, &mut |done, total| {
        on_progress(Progress::new(id, done, total.max(info.download_bytes), Status::Downloading))
    })?;
    on_progress(Progress::new(id, info.download_bytes, info.download_bytes, Status::Verifying));
    verify(&part, info.sha256)?;
    if info.archive {
        on_progress(Progress::new(id, info.download_bytes, info.download_bytes, Status::Extracting));
    }
    install_payload(root, id, &part)?;
    on_progress(Progress::new(id, info.download_bytes, info.download_bytes, Status::Done));
    Ok(())
}

pub fn uninstall(root: &Path, id: ModelId) -> Result<()> {
    let dir = catalog::model_dir(root, id);
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    let _ = fs::remove_file(part_path(root, id));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{is_installed, model_file};
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};
    use std::thread;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("diktator-dl-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn sha(bytes: &[u8]) -> String {
        hex(Sha256::digest(bytes).as_slice())
    }

    /// Minimal HTTP/1.1 server with Range support. If `cut_first_at` is set,
    /// the first response is cut off after that many body bytes.
    fn serve(body: Vec<u8>, cut_first_at: Option<usize>) -> (String, Arc<Mutex<Vec<u64>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let ranges = Arc::new(Mutex::new(Vec::new()));
        let seen = ranges.clone();
        thread::spawn(move || {
            let mut first = true;
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { continue };
                let mut req = Vec::new();
                let mut b = [0u8; 1];
                while !req.ends_with(b"\r\n\r\n") {
                    if s.read(&mut b).unwrap_or(0) == 0 {
                        break;
                    }
                    req.push(b[0]);
                }
                let text = String::from_utf8_lossy(&req).to_lowercase();
                let start: usize = text
                    .lines()
                    .find_map(|l| l.strip_prefix("range: bytes="))
                    .and_then(|r| r.trim().trim_end_matches('-').parse().ok())
                    .unwrap_or(0);
                seen.lock().unwrap().push(start as u64);
                let rest = &body[start..];
                let head = if start > 0 {
                    format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                        rest.len(), start, body.len() - 1, body.len()
                    )
                } else {
                    format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", rest.len())
                };
                let _ = s.write_all(head.as_bytes());
                let send = match (first, cut_first_at) {
                    (true, Some(cut)) => &rest[..cut],
                    _ => rest,
                };
                let _ = s.write_all(send);
                let _ = s.flush();
                first = false;
            }
        });
        (format!("http://{addr}/model.bin"), ranges)
    }

    #[test]
    fn downloads_and_verifies() {
        let body: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let (url, _) = serve(body.clone(), None);
        let dest = tmp("full").join("m.part");
        let mut last = (0, 0);
        fetch(&url, &dest, &AtomicBool::new(false), &mut |d, t| last = (d, t)).unwrap();
        verify(&dest, &sha(&body)).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), body);
        assert_eq!(last, (body.len() as u64, body.len() as u64));
    }

    #[test]
    fn checksum_mismatch_deletes_file() {
        let (url, _) = serve(b"hello".to_vec(), None);
        let dest = tmp("mismatch").join("m.part");
        fetch(&url, &dest, &AtomicBool::new(false), &mut |_, _| {}).unwrap();
        let err = verify(&dest, &"0".repeat(64)).unwrap_err();
        assert!(err.to_string().contains("Checksum mismatch"));
        assert!(!dest.exists());
    }

    #[test]
    fn resumes_after_interruption_with_range() {
        let body: Vec<u8> = (0..200_000u32).map(|i| (i % 253) as u8).collect();
        let (url, ranges) = serve(body.clone(), Some(80_000));
        let dest = tmp("resume").join("m.part");
        let cancel = AtomicBool::new(false);
        assert!(fetch(&url, &dest, &cancel, &mut |_, _| {}).is_err(), "first attempt is cut off");
        let partial = fs::metadata(&dest).unwrap().len();
        assert!(partial > 0 && partial < body.len() as u64);
        fetch(&url, &dest, &cancel, &mut |_, _| {}).unwrap();
        verify(&dest, &sha(&body)).unwrap();
        assert_eq!(ranges.lock().unwrap().as_slice(), &[0, partial]);
    }

    #[test]
    fn cancel_stops_and_keeps_partial() {
        let body = vec![7u8; 600_000];
        let (url, _) = serve(body, None);
        let dest = tmp("cancel").join("m.part");
        let cancel = AtomicBool::new(true);
        let err = fetch(&url, &dest, &cancel, &mut |_, _| {}).unwrap_err();
        assert!(err.downcast_ref::<Cancelled>().is_some());
    }

    #[test]
    fn installs_archive_atomically() {
        let root = tmp("install-archive");
        let info = ModelId::Canary180mFlash.info();
        let payload = root.join("canary.tar.bz2");
        {
            let enc = bzip2::write::BzEncoder::new(File::create(&payload).unwrap(), bzip2::Compression::fast());
            let mut builder = tar::Builder::new(enc);
            for f in info.files {
                let data = b"test";
                let mut header = tar::Header::new_gnu();
                header.set_size(data.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                builder.append_data(&mut header, f, &data[..]).unwrap();
            }
            builder.into_inner().unwrap().finish().unwrap();
        }
        install_payload(&root, ModelId::Canary180mFlash, &payload).unwrap();
        assert!(is_installed(&root, ModelId::Canary180mFlash));
        assert!(!payload.exists());
        assert!(!root.join(".staging-canary180m_flash").exists());
    }

    #[test]
    fn installs_single_file_and_uninstalls() {
        let root = tmp("install-file");
        let payload = root.join("vad.part");
        fs::write(&payload, b"onnx").unwrap();
        install_payload(&root, ModelId::SileroVad, &payload).unwrap();
        assert_eq!(fs::read(model_file(&root, ModelId::SileroVad, "silero_vad.onnx")).unwrap(), b"onnx");
        uninstall(&root, ModelId::SileroVad).unwrap();
        assert!(!is_installed(&root, ModelId::SileroVad));
    }

    #[test]
    fn archive_missing_expected_files_fails_cleanly() {
        let root = tmp("install-bad");
        let payload = root.join("bad.tar.bz2");
        {
            let enc = bzip2::write::BzEncoder::new(File::create(&payload).unwrap(), bzip2::Compression::fast());
            let mut builder = tar::Builder::new(enc);
            let mut header = tar::Header::new_gnu();
            header.set_size(1);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, "unrelated.txt", &b"x"[..]).unwrap();
            builder.into_inner().unwrap().finish().unwrap();
        }
        assert!(install_payload(&root, ModelId::Canary180mFlash, &payload).is_err());
        assert!(!is_installed(&root, ModelId::Canary180mFlash));
    }
}
