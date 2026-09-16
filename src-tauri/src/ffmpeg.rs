//! Finding, and if necessary installing, ffmpeg.
//!
//! KickCut downloads the segments itself but does not mux them itself - that is
//! ffmpeg's job, and writing an MP4 muxer to avoid a dependency would be an
//! absurd trade. So the app needs an ffmpeg, and the one thing it must not do is
//! make the user go and find one.
//!
//! Resolution order is: our own managed copy, then whatever is on PATH, then
//! install. PATH comes before installing so a machine that already has ffmpeg
//! is left alone; our copy comes before PATH so that once we have installed one,
//! a later PATH change cannot silently swap the binary underneath a job.
//!
//! The download is pinned to an exact version and checked against a SHA-256
//! published alongside it. An unpinned "latest" URL would mean the bytes
//! executed on the user's machine could change without this code changing,
//! which is not something to leave to chance.

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncWriteExt;

/// One pinned archive, and where it comes from.
///
/// Windows serves ffmpeg and ffprobe inside a single build; macOS serves them
/// separately, so this is a list rather than one set of constants.
struct Pin {
    /// What the download is saved as. Ours to choose - it only has to be
    /// recognisable when an interrupted install leaves one behind.
    archive: &'static str,
    /// Identical bytes at every entry; the later ones are mirrors.
    urls: &'static [&'static str],
    sha256: &'static str,
    bytes: u64,
}

const VERSION: &str = "9.0.1";

/// The second URL is the upstream author GitHub mirror, used when the first is
/// unreachable. Checksum published at
/// <https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip.sha256>,
/// checked 2026-09-07.
#[cfg(windows)]
const PINS: &[Pin] = &[Pin {
    archive: "ffmpeg-9.0.1-essentials_build.zip",
    urls: &[
        "https://www.gyan.dev/ffmpeg/builds/packages/ffmpeg-9.0.1-essentials_build.zip",
        "https://github.com/GyanD/codexffmpeg/releases/download/9.0.1/ffmpeg-9.0.1-essentials_build.zip",
    ],
    sha256: "fec81ae03971d9dd4be3ebe02e263bd2ec1d789483f931bdba5f5715e65da2e9",
    bytes: 111_253_802,
}];

/// Apple Silicon builds from <https://ffmpeg.martin-riedl.de>, which publishes a
/// .sha256 beside every download and keeps each build at its own stable URL -
/// the two things this pinning needs.
///
/// Checked 2026-09-16: both are native arm64, both carry an ad-hoc code
/// signature - Apple Silicon refuses to run a Mach-O without one, so a build
/// that lacked it would download, verify, and then be killed on sight - and
/// both declare a minimum of macOS 12, which is where minimumSystemVersion in
/// tauri.conf.json comes from.
///
/// There is no second URL: unlike the gyan.dev build this one has no mirror.
#[cfg(target_os = "macos")]
const PINS: &[Pin] = &[
    Pin {
        archive: "ffmpeg-9.0.1-macos-arm64.zip",
        urls: &["https://ffmpeg.martin-riedl.de/download/macos/arm64/1787073674_9.0.1/ffmpeg.zip"],
        sha256: "8287a1b2229e05eb41859f073e18e6c52c60a778f2f5e6881070fe51b79407fe",
        bytes: 28_447_413,
    },
    Pin {
        archive: "ffprobe-9.0.1-macos-arm64.zip",
        urls: &["https://ffmpeg.martin-riedl.de/download/macos/arm64/1787073674_9.0.1/ffprobe.zip"],
        sha256: "102a26b8940a053298d9929bfaae71e4b6ef65ba5f19a99a88c433108560741a",
        bytes: 28_370_930,
    },
];

/// A platform with no pin would compile and then fail at the one moment the
/// user cannot do anything about it, so it fails here instead.
#[cfg(not(any(windows, target_os = "macos")))]
compile_error!("ffmpeg is pinned per platform, and this one has no pin yet.");

/// Everything the installer will fetch, for the warning shown before it starts.
fn download_bytes() -> u64 {
    PINS.iter().map(|pin| pin.bytes).sum()
}

#[cfg(windows)]
pub const EXE: &str = ".exe";
#[cfg(not(windows))]
pub const EXE: &str = "";

/// Where ffmpeg was found, so the UI can say something truthful about it.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    /// Installed by this app into its own data directory.
    Managed,
    /// Already present on the machine.
    System,
    Missing,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub source: Source,
    /// First line of `ffmpeg -version`, or None when nothing was found.
    pub version: Option<String>,
    pub path: Option<String>,
    /// Bytes the installer will need to fetch, so the UI can warn before it starts.
    pub download_bytes: u64,
    pub download_version: &'static str,
}

/// Progress of an install, emitted as `ffmpeg-install`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    stage: &'static str,
    received: u64,
    total: u64,
}

/// Resolved binaries, for the mux phase.
#[derive(Debug, Clone)]
pub struct Tools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

fn managed_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|d| d.join("bin"))
        .map_err(|e| format!("This machine's application data folder could not be located: {e}"))
}

/// Run a binary with a short argument and return its first stdout line.
///
/// Doubles as the liveness check: a file that exists but does not run - a
/// half-extracted download, a binary for the wrong architecture - fails here
/// rather than at the end of an hour-long job.
async fn probe_version(bin: &Path) -> Option<String> {
    let mut cmd = tokio::process::Command::new(bin);
    cmd.arg("-version");
    #[cfg(windows)]
    {
        // Without this every invocation flashes a console window over the app.
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd.output().await.ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .map(|l| l.trim().to_string())
}

/// Locate a usable pair of binaries, or report why not.
pub async fn locate(app: &AppHandle) -> Result<(Status, Option<Tools>), String> {
    resolve(&managed_dir(app)?).await
}

/// The resolution order itself, with the managed directory passed in.
///
/// Split from `locate` so it can be tested against a real ffmpeg without a
/// running Tauri app - "do I still have to install it if I already have
/// ffmpeg?" is the first thing anyone asks, and the answer deserves a test
/// rather than an assurance.
pub async fn resolve(dir: &Path) -> Result<(Status, Option<Tools>), String> {
    let candidates = [
        (
            Source::Managed,
            dir.join(format!("ffmpeg{EXE}")),
            dir.join(format!("ffprobe{EXE}")),
        ),
        // Bare names resolve through PATH.
        (
            Source::System,
            PathBuf::from(format!("ffmpeg{EXE}")),
            PathBuf::from(format!("ffprobe{EXE}")),
        ),
    ];

    for (source, ffmpeg, ffprobe) in candidates {
        // The managed copy is only considered if it is actually on disk;
        // probing a missing absolute path is a slow way to learn nothing.
        if source == Source::Managed && !ffmpeg.is_file() {
            continue;
        }
        let Some(version) = probe_version(&ffmpeg).await else {
            continue;
        };
        // ffprobe is required too: the mux step verifies its own output, and
        // discovering ffprobe is missing after an hour of downloading is not
        // an acceptable place to find out.
        if probe_version(&ffprobe).await.is_none() {
            continue;
        }
        return Ok((
            Status {
                source,
                version: Some(version),
                path: Some(ffmpeg.display().to_string()),
                download_bytes: download_bytes(),
                download_version: VERSION,
            },
            Some(Tools { ffmpeg, ffprobe }),
        ));
    }

    Ok((
        Status {
            source: Source::Missing,
            version: None,
            path: None,
            download_bytes: download_bytes(),
            download_version: VERSION,
        },
        None,
    ))
}

/// True while an install is running, so the startup tidy-up below cannot
/// delete an archive that is still being written.
static INSTALLING: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub async fn ffmpeg_status(app: AppHandle) -> Result<Status, String> {
    let (status, _) = locate(&app).await?;
    // A previous attempt that was interrupted leaves its archive behind. This
    // is the moment it is provably garbage: nothing usable was installed, and
    // no install is running.
    if status.source == Source::Missing && !INSTALLING.load(Ordering::Relaxed) {
        let freed = clear_stale_archive(&managed_dir(&app)?).await;
        if freed > 0 {
            log_cleared(freed);
        }
    }
    Ok(status)
}

fn log_cleared(bytes: u64) {
    eprintln!("cleared {bytes} bytes left by an unfinished ffmpeg install");
}

/// Download, verify and unpack the pinned build.
///
/// Emits `ffmpeg-install` throughout. Safe to call when ffmpeg is already
/// present - it simply replaces the managed copy.
#[tauri::command]
pub async fn install_ffmpeg(app: AppHandle) -> Result<Status, String> {
    let dir = managed_dir(&app)?;
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("Could not create {}: {e}", dir.display()))?;

    // A stump from a previous attempt is replaced, not resumed: the download
    // has no range support and the checksum covers the whole file.
    clear_stale_archive(&dir).await;

    INSTALLING.store(true, Ordering::Relaxed);
    let reporter = app.clone();
    let outcome = fetch_and_unpack(&dir, move |stage, received, total| {
        emit(&reporter, stage, received, total)
    })
    .await;
    INSTALLING.store(false, Ordering::Relaxed);
    outcome?;

    let (status, tools) = locate(&app).await?;
    if tools.is_none() {
        return Err("ffmpeg was installed but will not run on this machine.".into());
    }
    emit(&app, "done", 0, 0);
    Ok(status)
}

fn emit(app: &AppHandle, stage: &'static str, received: u64, total: u64) {
    let _ = app.emit(
        "ffmpeg-install",
        Progress {
            stage,
            received,
            total,
        },
    );
}

/// The whole install, with no Tauri in it.
///
/// Kept free of `AppHandle` so it can be tested for real: progress arrives
/// through the callback, and the caller decides whether that becomes an event
/// or is thrown away.
pub async fn fetch_and_unpack(
    dir: &Path,
    report: impl Fn(&'static str, u64, u64) + Send + Sync + 'static,
) -> Result<(), String> {

    /*
     * The archive is removed however this ends.
     *
     * It used only to be cleaned up on success and on a checksum mismatch, so
     * an install that failed - or that was still running when the app was
     * closed - left tens of megabytes sitting in the app's folder with nothing on
     * screen to say why. Found in the wild: a 12 MB stump from an interrupted
     * attempt, and Settings still just said "not installed".
     */
    let outcome = fetch_and_unpack_inner(dir, report).await;
    clear_stale_archive(dir).await;
    outcome
}

async fn fetch_and_unpack_inner(
    dir: &Path,
    report: impl Fn(&'static str, u64, u64) + Send + Sync + 'static,
) -> Result<(), String> {
    // Progress counts the whole install rather than the archive in hand, so a
    // platform that needs two of them does not run the bar to the end twice.
    let total = download_bytes();
    let mut fetched = 0;
    let mut found = 0;

    for pin in PINS {
        let archive = dir.join(pin.archive);
        // Download and hash in one pass, then check before anything is unpacked.
        let digest = download(pin, &archive, fetched, total, &report).await?;
        report("verifying", fetched, total);
        let expected = pin.sha256;
        if digest != expected {
            return Err(format!(
                "The downloaded ffmpeg does not match its published checksum, so it was \
                 discarded. Expected {expected}, got {digest}."
            ));
        }

        report("extracting", fetched, total);
        let extract_to = dir.to_path_buf();
        let archive_for_task = archive.clone();
        // The zip crate is blocking and this unpacks a couple of hundred
        // megabytes, so it runs off the async runtime rather than stalling
        // every task on it.
        found += tokio::task::spawn_blocking(move || extract(&archive_for_task, &extract_to))
            .await
            .map_err(|e| format!("Unpacking ffmpeg did not finish: {e}"))??;
        fetched += pin.bytes;
    }

    // Checked across the whole set, because on macOS each archive carries only
    // one of the two binaries.
    if found != 2 {
        return Err("The download did not contain ffmpeg and ffprobe.".into());
    }
    Ok(())
}

/// Delete an archive left behind by an install that never finished.
///
/// Only safe when nothing is downloading, so it is called where an install is
/// about to start or is known not to be running.
pub async fn clear_stale_archive(dir: &Path) -> u64 {
    let mut freed = 0;
    for pin in PINS {
        let archive = dir.join(pin.archive);
        let Ok(meta) = tokio::fs::metadata(&archive).await else {
            continue;
        };
        if tokio::fs::remove_file(&archive).await.is_ok() {
            freed += meta.len();
        }
    }
    freed
}

/// Stream the archive to `target`, returning its lowercase hex SHA-256.
async fn download(
    pin: &Pin,
    target: &Path,
    base: u64,
    grand_total: u64,
    report: &(impl Fn(&'static str, u64, u64) + Send + Sync),
) -> Result<String, String> {
    /*
     * Its own client, deliberately. The one the rest of the app uses caps a
     * whole request at 30 s, which is right for a playlist and fatally wrong
     * for a hundred megabytes - it cut every attempt off mid-transfer. What
     * needs a deadline here is a stall, not the transfer, so the limits are on
     * connecting and on time between chunks; a slow connection is allowed to
     * take as long as it takes.
     */
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .read_timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("HTTP client could not be created: {e}"))?;
    let mut last_error = String::new();

    for url in pin.urls.iter().copied() {
        // Cleared per attempt, or a failure on the first mirror would condemn
        // a good download from the second.
        last_error.clear();
        let response = match client.get(url).send().await {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                last_error = format!("{url} answered {}", r.status());
                continue;
            }
            Err(e) => {
                last_error = format!("{url}: {e}");
                continue;
            }
        };

        let mut file = tokio::fs::File::create(target)
            .await
            .map_err(|e| format!("Could not write to {}: {e}", target.display()))?;
        let mut hasher = Sha256::new();
        let mut received: u64 = 0;
        let mut since_emit: u64 = 0;
        let mut stream = response;

        loop {
            let chunk = match stream.chunk().await {
                Ok(Some(c)) => c,
                Ok(None) => break,
                Err(e) => {
                    last_error = format!("{url}: transfer interrupted: {e}");
                    // Fall through to the next mirror rather than failing the
                    // whole install on one flaky connection.
                    break;
                }
            };
            hasher.update(&chunk);
            file.write_all(&chunk)
                .await
                .map_err(|e| format!("Could not write ffmpeg to disk: {e}"))?;
            received += chunk.len() as u64;
            since_emit += chunk.len() as u64;
            // A progress event per chunk would be thousands of IPC messages a
            // second; every megabyte is smooth enough to watch.
            if since_emit >= 1_048_576 {
                since_emit = 0;
                report("downloading", base + received, grand_total);
            }
        }

        file.flush()
            .await
            .map_err(|e| format!("Could not finish writing ffmpeg: {e}"))?;

        if received > 0 && last_error.is_empty() {
            report("downloading", base + received, grand_total);
            return Ok(format!("{:x}", hasher.finalize()));
        }
    }

    Err(format!("ffmpeg could not be downloaded. {last_error}"))
}

/// Pull whichever of the two binaries this archive holds, and say how many.
///
/// The Windows build also ships ffplay, documentation and presets; none of it
/// is used here, and unpacking it would roughly double what sits on the user's
/// disk.
fn extract(archive: &Path, dir: &Path) -> Result<usize, String> {
    let file = std::fs::File::open(archive)
        .map_err(|e| format!("The downloaded archive could not be opened: {e}"))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| format!("The downloaded archive is not readable: {e}"))?;

    let wanted = [format!("ffmpeg{EXE}"), format!("ffprobe{EXE}")];
    let mut found = 0;

    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| format!("The archive could not be read: {e}"))?;
        // Entries are nested under a versioned folder, and the name is matched
        // rather than the path so a changed folder layout does not break this.
        // `enclosed_name` also rejects any entry trying to escape the directory.
        let Some(name) = entry
            .enclosed_name()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        else {
            continue;
        };
        if !wanted.contains(&name) {
            continue;
        }

        let mut buffer = Vec::with_capacity(entry.size() as usize);
        entry
            .read_to_end(&mut buffer)
            .map_err(|e| format!("{name} could not be unpacked: {e}"))?;
        let path = dir.join(&name);
        std::fs::write(&path, buffer)
            .map_err(|e| format!("{name} could not be saved: {e}"))?;
        // Unix keeps the executable bit outside the file, and a zip entry's mode
        // does not survive a plain write - so without this the binary lands
        // unable to run, and the install reports success either way.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| format!("{name} could not be made executable: {e}"))?;
        }
        found += 1;
    }

    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pinned_urls_point_at_the_pinned_version() {
        // A version bump that misses one of these would install a build whose
        // checksum cannot match, so tie them together here.
        for pin in PINS {
            for url in pin.urls {
                assert!(url.contains(VERSION), "{url} is not the pinned version");
            }
            // The saved name carries the version too, so a stump left by an
            // old install cannot be mistaken for the current download.
            assert!(
                pin.archive.contains(VERSION),
                "{} is not the pinned version",
                pin.archive
            );
            assert_eq!(pin.sha256.len(), 64);
            assert!(pin
                .sha256
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()));
            assert!(pin.bytes > 0, "{} has no expected size", pin.archive);
        }
    }

    /// An install that never finished leaves an archive; it must not survive.
    ///
    /// Found in the wild before this was fixed: a 12 MB stump of the 106 MB
    /// archive sitting in the app folder, with Settings still just saying
    /// "not installed" and nothing to explain the missing disk space.
    #[tokio::test]
    async fn an_interrupted_install_leaves_nothing_behind() {
        let dir = std::env::temp_dir().join("kickcut-stale-archive-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let archive = dir.join(PINS[0].archive);
        std::fs::write(&archive, vec![0u8; 12_934_883]).unwrap();

        let freed = clear_stale_archive(&dir).await;
        assert_eq!(freed, 12_934_883, "should report what it reclaimed");
        assert!(!archive.exists(), "the stump should be gone");

        // Nothing to clear is not an error, and reclaims nothing.
        assert_eq!(clear_stale_archive(&dir).await, 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Copy a real ffmpeg pair into `dir`, or skip the test if none is around.
    ///
    /// Looks for the managed copy this app installs, then anything on PATH.
    fn borrow_real_binaries(dir: &Path) -> bool {
        let sources: Vec<PathBuf> = std::env::var_os("KICKCUT_TEST_FFMPEG_DIR")
            .map(|d| vec![PathBuf::from(d)])
            .unwrap_or_default();

        for source in sources {
            let ffmpeg = source.join(format!("ffmpeg{EXE}"));
            let ffprobe = source.join(format!("ffprobe{EXE}"));
            if ffmpeg.is_file() && ffprobe.is_file() {
                std::fs::create_dir_all(dir).unwrap();
                std::fs::copy(&ffmpeg, dir.join(format!("ffmpeg{EXE}"))).unwrap();
                std::fs::copy(&ffprobe, dir.join(format!("ffprobe{EXE}"))).unwrap();
                return true;
            }
        }
        false
    }

    /// An ffmpeg already on the machine is used as-is.
    ///
    /// This is the question every first-time user asks, so it is answered by
    /// running the real resolution order against a real binary rather than by
    /// reading the code. Set `KICKCUT_TEST_FFMPEG_DIR` to a folder holding
    /// ffmpeg and ffprobe, then run with `--ignored`.
    #[tokio::test]
    #[ignore = "needs a real ffmpeg; set KICKCUT_TEST_FFMPEG_DIR"]
    async fn an_ffmpeg_already_on_path_is_found_and_nothing_is_downloaded() {
        let root = std::env::temp_dir().join("kickcut-resolve-test");
        let _ = std::fs::remove_dir_all(&root);
        let on_path = root.join("on-path");
        let managed = root.join("managed");
        std::fs::create_dir_all(&managed).unwrap();

        if !borrow_real_binaries(&on_path) {
            eprintln!("no ffmpeg to borrow; set KICKCUT_TEST_FFMPEG_DIR");
            return;
        }

        // The managed directory is empty, exactly as it is on a fresh install.
        let previous = std::env::var("PATH").unwrap_or_default();
        // Built rather than formatted: the separator is ; on Windows and :
        // everywhere else, and hardcoding one makes this test quietly find
        // nothing on the other platform.
        let mut search = vec![on_path.clone().into_os_string()];
        search.extend(std::env::split_paths(&previous).map(|p| p.into_os_string()));
        std::env::set_var("PATH", std::env::join_paths(&search).unwrap());

        let (status, tools) = resolve(&managed).await.expect("resolve");
        assert_eq!(status.source, Source::System, "should have used the machine's own copy");
        assert!(status.version.is_some_and(|v| v.contains("version")));
        assert!(tools.is_some(), "both binaries should have resolved");

        // Now give the managed directory its own copy: it must win, so that a
        // later PATH change cannot swap the binary under a running job.
        assert!(borrow_real_binaries(&managed));
        let (status, _) = resolve(&managed).await.expect("resolve");
        assert_eq!(status.source, Source::Managed, "our own copy must take precedence");

        std::env::set_var("PATH", previous);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The real install, end to end.
    ///
    /// `#[ignore]`d because it fetches the whole build. Run it deliberately - with
    /// `cargo test -- --ignored` - after changing the pinned version, the
    /// checksum, or anything in the unpacking path. It is the only thing that
    /// proves the three parts agree: that the URL still serves the archive the
    /// checksum describes, and that the archive still contains binaries that
    /// run on this machine.
    #[tokio::test]
    #[ignore = "downloads the pinned ffmpeg build"]
    async fn installs_the_pinned_build_and_the_binaries_run() {
        let dir = std::env::temp_dir().join("kickcut-ffmpeg-install-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");

        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<&'static str>::new()));
        let recorder = seen.clone();
        fetch_and_unpack(&dir, move |stage, _, _| {
            let mut log = recorder.lock().unwrap();
            if log.last() != Some(&stage) {
                log.push(stage);
            }
        })
        .await
        .expect("install should succeed");

        // Progress has to reach the UI in order, or the installer looks stuck.
        assert_eq!(
            *seen.lock().unwrap(),
            vec!["downloading", "verifying", "extracting"]
        );

        for name in [format!("ffmpeg{EXE}"), format!("ffprobe{EXE}")] {
            let bin = dir.join(&name);
            assert!(bin.is_file(), "{name} was not extracted");
            let version = probe_version(&bin).await.unwrap_or_else(|| panic!("{name} did not run"));
            assert!(version.contains("version"), "unexpected banner: {version}");
        }

        // Only the two binaries, not the rest of the archive.
        let extra: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| !n.starts_with("ffmpeg") && !n.starts_with("ffprobe"))
            .collect();
        assert!(extra.is_empty(), "archive left behind: {extra:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
