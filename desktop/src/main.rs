
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};
use std::net::IpAddr;

mod ui;
mod design;
mod watch;
mod internet;

const MAX_SIZE: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
struct Settings {
    address: String,
    token: String,
    #[serde(default)]
    recipient_id: String,
    #[serde(default = "default_auto_send")]
    auto_send: bool,
    #[serde(default)]
    watch_folder: String,
    #[serde(default)]
    watch_enabled: bool,
}

fn default_auto_send() -> bool { true }

impl Default for Settings {
    fn default() -> Self {
        Self { address: String::new(), token: String::new(), recipient_id: String::new(), auto_send: true, watch_folder: String::new(), watch_enabled: false }
    }
}

fn config_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("Synchro").join("settings.json"))
}

// Unsent files are kept separately from credentials and restored after a restart.
fn queue_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("Synchro").join("pending-queue.json"))
}

fn load_queue() -> Vec<PathBuf> {
    let Some(path) = queue_path() else { return Vec::new(); };
    let mut candidates = vec![path.clone()];
    candidates.push(path.with_extension("bak"));
    for candidate in candidates {
        if let Ok(contents) = std::fs::read_to_string(candidate) {
            if let Ok(mut items) = serde_json::from_str::<Vec<PathBuf>>(&contents) {
                items.truncate(500);
                items.dedup();
                return items;
            }
        }
    }
    Vec::new()
}

fn save_queue(files: &[PathBuf]) -> Result<(), String> {
    let path = queue_path().ok_or("Cannot locate the queue directory")?;
    let parent = path.parent().ok_or("Invalid queue path")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let payload = serde_json::to_vec_pretty(files).map_err(|e| e.to_string())?;
    let temp = path.with_extension("tmp");
    std::fs::write(&temp, payload).map_err(|e| e.to_string())?;
    if path.exists() {
        // Windows std::fs::rename cannot always overwrite an existing target.
        // Keep the previous valid file as a recovery fallback.
        let _ = std::fs::copy(&path, path.with_extension("bak"));
    }
    if std::fs::rename(&temp, &path).is_err() {
        std::fs::copy(&temp, &path).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(temp);
    }
    Ok(())
}

fn load_settings() -> Settings {
    config_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_settings(s: &Settings) -> Result<(), String> {
    let path = config_path().ok_or("Can't find user config directory")?;
    std::fs::create_dir_all(path.parent().ok_or("Invalid config path")?)
        .map_err(|e| e.to_string())?;
    let content = serde_json::to_vec_pretty(s).map_err(|e| e.to_string())?;
    std::fs::write(path, content).map_err(|e| e.to_string())
}

fn target_url(address: &str) -> Result<String, String> {
    let value = address.trim().trim_end_matches('/');
    let url = reqwest::Url::parse(value).map_err(|_| "Enter http://PHONE_IP:8765".to_string())?;
    if url.scheme() != "http" || url.host_str().is_none() || url.port().is_none()
        || !url.username().is_empty() || url.password().is_some()
        || url.path() != "/" || url.query().is_some() || url.fragment().is_some()
    {
        return Err("Use only http://PHONE_IP:PORT (no path or credentials)".into());
    }
    // Pairing tokens must not be leaked to arbitrary public HTTP servers.
    // Trusted private LAN, loopback (ADB forward), and Tailscale CGNAT are supported.
    let hostname = url.host_str().unwrap_or_default();
    let host = hostname.trim_start_matches('[').trim_end_matches(']');
    let ip: IpAddr = host.parse().map_err(|_| {
        "Використовуй IP-адресу Wi-Fi, Tailscale або 127.0.0.1 (без публічного домену)".to_string()
    })?;
    let allowed = match ip {
        IpAddr::V4(addr) => {
            let octets = addr.octets();
            addr.is_private() || addr.is_loopback() ||
                (octets[0] == 100 && (64..=127).contains(&octets[1]))
        }
        IpAddr::V6(addr) => addr.is_loopback() || addr.is_unique_local()
            || addr.is_unicast_link_local(),
    };
    if !allowed {
        return Err("Небезпечна публічна HTTP-адреса. Використовуй LAN або Tailscale.".into());
    }
    Ok(format!("{value}/upload"))
}

#[derive(Clone, Debug, Deserialize)]
struct DeviceInfo {
    #[serde(default)]
    device: String,
    #[serde(default, rename = "appVersion")]
    app_version: String,
    #[serde(default)]
    sdk: u32,
}
#[derive(Clone)]
struct SentItem {
    name: String,
    size: u64,
}

#[derive(Debug)]
enum Event {
    Status(String),
    Progress(PathBuf, u64, u64),
    Finished(Result<String, String>),
    FileSent(PathBuf),
    FilesPicked(Vec<PathBuf>),
    FolderPicked(PathBuf),
    WatchFile(PathBuf),
    Connected(String, String, Result<DeviceInfo, String>),
}

struct ProgressFile {
    file: File,
    path: PathBuf,
    sent: u64,
    total: u64,
    events: Sender<Event>,
    last_report: u64,
}

impl Read for ProgressFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let count = self.file.read(buf)?;
        self.sent += count as u64;
        if self.sent.saturating_sub(self.last_report) > 256 * 1024 || self.sent == self.total {
            self.last_report = self.sent;
            let _ = self.events.send(Event::Progress(self.path.clone(), self.sent, self.total));
        }
        Ok(count)
    }
}

fn hex_digest(file: &Path) -> Result<String, String> {
    let mut input = File::open(file).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = input.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 { break; }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn check_receiver(settings: &Settings) -> Result<DeviceInfo, String> {
    if settings.address.trim().starts_with("https://") { return internet::probe(settings); }
    if settings.token.trim().len() != 32 || !settings.token.trim().bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Спочатку введи правильний код сполучення (32 символи).".into());
    }
    let upload_url = target_url(&settings.address)?;
    let health_url = upload_url.trim_end_matches("upload").to_owned() + "health";
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build().map_err(|e| e.to_string())?;
    let response = client.get(health_url)
        .header("X-Synchro-Token", settings.token.trim())
        .send().map_err(|e| format!("Телефон не відповідає: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("Android повернув HTTP {}. Перевір код і версію Android Synchro.",
            response.status()));
    }
    let body = response.text().map_err(|e| format!("Недійсна відповідь Android: {e}"))?;
    let mut info: DeviceInfo = serde_json::from_str(&body)
        .map_err(|e| format!("Некоректна відповідь Android: {e}"))?;
    if info.device.is_empty() {
        info.device = "Android-пристрій".into();
    }
    Ok(info)
}

fn send_files(files: Vec<PathBuf>, settings: Settings, tx: Sender<Event>) -> Result<String, String> {
    if settings.address.trim().starts_with("https://") { return internet::send(files, settings, tx); }
    if settings.token.trim().len() != 32
        || !settings.token.trim().bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("Enter the 32-character pairing token shown on Android".into());
    }
    let url = target_url(&settings.address)?;
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(30 * 60))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    let mut sent_files = 0;
    for path in files {
        let name = path.file_name().and_then(|s| s.to_str())
            .ok_or_else(|| format!("Invalid file name: {}", path.display()))?;
        let metadata = path.metadata().map_err(|e| e.to_string())?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_SIZE {
            return Err(format!("File must be 1 byte to 2 GiB: {name}"));
        }
        let _ = tx.send(Event::Status(format!("Checking SHA-256: {name}")));
        let sha256 = hex_digest(&path)?;
        let checked = path.metadata().map_err(|e| e.to_string())?;
        if checked.len() != metadata.len() || checked.modified().ok() != metadata.modified().ok() {
            return Err(format!("{name}: build output changed while computing SHA-256; retry after build completes"));
        }
        let file = File::open(&path).map_err(|e| e.to_string())?;
        let reader = ProgressFile {
            file, path: path.clone(), sent: 0, total: metadata.len(), events: tx.clone(), last_report: 0,
        };
        let _ = tx.send(Event::Status(format!("Uploading: {name}")));
        let response = client.post(&url)
            .header("X-Synchro-Token", settings.token.trim())
            .header("X-Synchro-Name", utf8_percent_encode(name, NON_ALPHANUMERIC).to_string())
            .header("X-Synchro-Sha256", sha256)
            .header("Content-Type", "application/octet-stream")
            .body(reqwest::blocking::Body::sized(reader, metadata.len()))
            .send()
            .map_err(|e| format!("{name}: {e}"))?;
        let code = response.status();
        if !code.is_success() {
            let reason = response.text().unwrap_or_default();
            return Err(format!("{name}: Android rejected upload ({code}): {reason}"));
        }
        sent_files += 1;
        // Android confirmed THIS byte stream. A build may have overwritten the same
        // app-debug.apk path during upload; leave the newer version queued.
        let still_same = path.metadata().map(|current| {
            current.len() == metadata.len() &&
            current.modified().ok() == metadata.modified().ok()
        }).unwrap_or(false);
        if still_same {
            let _ = tx.send(Event::FileSent(path.clone()));
        } else {
            let _ = tx.send(Event::Status(format!(
                "{name}: a new build appeared during upload; newest file remains in queue"
            )));
        }
    }
    Ok(format!("Sent {sent_files} file(s) to Android · Download/Synchro/Debug"))
}

struct SynchroApp {
    settings: Settings,
    files: Vec<PathBuf>,
    manual_path: String,
    status: String,
    progress: f32,
    busy: bool,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    page: ui::Page,
    connection: ui::Connection,
    checking: bool,
    last_checked_pair: Option<(String, String)>,
    show_token: bool,
    show_manual_path: bool,
    history: Vec<SentItem>,
    current_path: Option<PathBuf>,
    watcher: watch::FolderWatch,
    applied_watch_enabled: bool,
    applied_watch_folder: String,
    last_check: Instant,
}

impl Default for SynchroApp {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        let settings = load_settings();
        Self {
            applied_watch_enabled: settings.watch_enabled,
            applied_watch_folder: settings.watch_folder.clone(),
            settings, files: load_queue(), manual_path: String::new(),
            status: "Pair Android, then drop APK/ZIP files here".into(),
            progress: 0.0, busy: false, tx, rx,
            page: ui::Page::default(), connection: ui::Connection::default(),
            checking: false, last_checked_pair: None,
            show_token: false, show_manual_path: false,
            history: Vec::new(), current_path: None,
            watcher: watch::FolderWatch::default(),
            last_check: Instant::now() - Duration::from_secs(30),
        }
    }
}

impl SynchroApp {
    fn add_path(&mut self, path: PathBuf) {
        if path.is_file() && !self.files.contains(&path) {
            self.files.push(path);
        } else if !path.is_file() {
            self.status = "Drag a file, not a directory".into();
        }
    }
    fn persist_settings(&mut self) {
        match save_settings(&self.settings) {
            Ok(()) => {
                self.applied_watch_enabled = self.settings.watch_enabled;
                self.applied_watch_folder = self.settings.watch_folder.clone();
                self.sync_watcher();
                self.status = "Налаштування збережено.".into();
            },
            Err(e) => self.status = format!("Не вдалося зберегти налаштування: {e}"),
        }
    }

    fn choose_files(&mut self) {
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            if let Some(files) = rfd::FileDialog::new()
                .set_title("Вибери файли для Synchro")
                .pick_files()
            {
                let _ = tx.send(Event::FilesPicked(files));
            }
        });
    }

    fn check_connection(&mut self) {
        if self.checking { return; }
        self.last_check = Instant::now();
        self.checking = true;
        self.connection = ui::Connection::Unknown;
        self.last_checked_pair = Some((self.settings.address.clone(), self.settings.token.clone()));
        let settings = Settings {
            address: self.settings.address.clone(),
            token: self.settings.token.clone(),
            recipient_id: self.settings.recipient_id.clone(),
            auto_send: self.settings.auto_send,
            watch_folder: self.settings.watch_folder.clone(),
            watch_enabled: self.settings.watch_enabled,
        };
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = check_receiver(&settings);
            let _ = tx.send(Event::Connected(settings.address, settings.token, result));
        });
    }

    fn choose_watch_folder(&mut self) {
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                let _ = tx.send(Event::FolderPicked(folder));
            }
        });
    }

    fn sync_watcher(&mut self) {
        self.watcher.configure(
            self.applied_watch_enabled,
            &self.applied_watch_folder,
            self.tx.clone(),
        );
    }

    fn start(&mut self) {
        if self.busy || self.files.is_empty() { return; }
        let destination = if self.settings.address.trim().starts_with("https://") {
            internet::origin(&self.settings.address)
        } else { target_url(&self.settings.address) };
        if let Err(e) = destination {
            self.status = e;
            return;
        }
        if let Err(e) = save_settings(&self.settings) {
            self.status = format!("Can't save pairing: {e}");
            return;
        }
        self.busy = true;
        self.progress = 0.0;
        let files = self.files.clone(); // Remove each item only after Android confirms receipt.
        let settings = Settings {
            address: self.settings.address.clone(), token: self.settings.token.clone(),
            recipient_id: self.settings.recipient_id.clone(),
            auto_send: self.settings.auto_send,
            watch_folder: self.settings.watch_folder.clone(),
            watch_enabled: self.settings.watch_enabled,
        };
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = send_files(files, settings, tx.clone());
            let _ = tx.send(Event::Finished(result));
        });
    }
}

impl eframe::App for SynchroApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let queue_before = self.files.clone();
        let mut new_files = false;
        let mut continue_queue = false;
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Status(s) => {
                    self.status = s;
                    self.progress = 0.0;
                }
                Event::Progress(path, done, total) => {
                    self.current_path = Some(path);
                    if total > 0 { self.progress = done as f32 / total as f32; }
                }
                Event::Finished(result) => {
                    self.busy = false;
                    self.current_path = None;
                    continue_queue = result.is_ok() && self.settings.auto_send;
                    if result.is_err() {
                        // Re-probe after a failed transfer instead of presenting stale Online.
                        self.last_check = Instant::now() - Duration::from_secs(30);
                        self.connection = ui::Connection::Unknown;
                    }
                    self.status = result.unwrap_or_else(|e| format!("Помилка передачі: {e}"));
                }
                Event::FileSent(path) => {
                    if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                        let size = path.metadata().map(|m| m.len()).unwrap_or(0);
                        self.history.insert(0, SentItem { name: name.to_owned(), size });
                        self.history.truncate(15);
                    }
                    self.files.retain(|f| f != &path);
                }
                Event::FolderPicked(folder) => {
                    self.settings.watch_folder = folder.to_string_lossy().into_owned();
                    self.persist_settings();
                }
                Event::WatchFile(path) => {
                    if !self.files.contains(&path) && path.is_file() {
                        self.add_path(path);
                        new_files = true;
                    }
                }
                Event::FilesPicked(files) => {
                    for file in files {
                        self.add_path(file);
                        new_files = true;
                    }
                }
                Event::Connected(address, token, result) => {
                    self.checking = false;
                    if self.settings.address == address && self.settings.token == token {
                        self.connection = match result {
                            Ok(info) => ui::Connection::Online(info),
                            Err(e) => ui::Connection::Offline(e),
                        };
                    } else {
                        self.connection = ui::Connection::Unknown;
                    }
                }
            }
        }
        // A new IP or pairing token invalidates an older successful connection check.
        if let Some((address, token)) = &self.last_checked_pair {
            if self.settings.address != *address || self.settings.token != *token {
                self.connection = ui::Connection::Unknown;
            }
        }
        let dropped: Vec<PathBuf> = ui.input(|input| input.raw.dropped_files.iter()
            .map(|f| f.path().to_path_buf()).collect());
        for file in dropped {
            self.add_path(file);
            new_files = true;
        }
        if new_files && self.settings.auto_send && !self.busy &&
            !self.settings.address.trim().is_empty() && !self.settings.token.trim().is_empty() {
            self.start();
        }
        if !self.checking && !self.busy
            && !self.settings.address.trim().is_empty()
            && !self.settings.token.trim().is_empty()
            && self.last_check.elapsed() > Duration::from_secs(20)
        {
            self.check_connection();
        }
        self.sync_watcher();
        // Render the real transfer state and controls; no simulated phones or QR codes.
        ui::render(ui, self);
        if self.files != queue_before {
            if let Err(e) = save_queue(&self.files) {
                self.status = format!("Не вдалося зберегти чергу: {e}");
            }
        }
        if continue_queue && !self.busy && !self.files.is_empty() {
            self.start();
        }
        ui.ctx().request_repaint_after(Duration::from_millis(if self.busy || self.checking { 100 } else { 400 }));
    }
}

fn main() -> eframe::Result {
    eframe::run_native(
        "Synchro", eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([1180.0, 790.0])
                .with_min_inner_size([850.0, 620.0])
                .with_drag_and_drop(true),
            ..Default::default()
        },
        Box::new(|cc| {
            ui::configure_theme(&cc.egui_ctx);
            Ok(Box::<SynchroApp>::default())
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn valid_host() {
        assert_eq!(target_url("http://192.168.1.10:8765").unwrap(), "http://192.168.1.10:8765/upload");
    }
    #[test]
    fn reject_url_with_credentials() {
        assert!(target_url("http://user:pass@localhost:8765").is_err());
    }
    #[test]
    fn reject_https_path() {
        assert!(target_url("https://example.com:8765/path").is_err());
    }
    #[test]
    fn tailscale_and_adb_hosts() {
        assert!(target_url("http://100.111.22.33:8765").is_ok());
        assert!(target_url("http://127.0.0.1:8765").is_ok());
    }
    #[test]
    fn reject_public_or_untrusted_host() {
        assert!(target_url("http://8.8.8.8:8765").is_err());
        assert!(target_url("http://example.com:8765").is_err());
        assert!(target_url("http://evil.com:8765/").is_err());
    }
    #[test]
    fn pending_queue_serialization_preserves_multiple_build_paths() {
        let paths = vec![
            PathBuf::from(r"M:\projects\alpha\app-debug.apk"),
            PathBuf::from(r"M:\projects\beta\app-debug.apk"),
        ];
        let data = serde_json::to_vec_pretty(&paths).unwrap();
        let parsed: Vec<PathBuf> = serde_json::from_slice(&data).unwrap();
        assert_eq!(parsed, paths);
    }

    #[test]
    fn decode_device_health() {
        let info: DeviceInfo = serde_json::from_str(
            r#"{"message":"ready","device":"Google Pixel 8","appVersion":"0.3.0","sdk":35}"#
        ).unwrap();
        assert_eq!(info.device, "Google Pixel 8");
        assert_eq!(info.sdk, 35);
    }
}
