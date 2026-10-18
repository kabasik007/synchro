
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

mod ui;

const MAX_SIZE: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Default, Serialize, Deserialize)]
struct Settings {
    address: String,
    token: String,
    #[serde(default = "default_auto_send")]
    auto_send: bool,
}

fn default_auto_send() -> bool { true }

fn config_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("Synchro").join("settings.json"))
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
    Ok(format!("{value}/upload"))
}

#[derive(Debug)]
enum Event {
    Status(String),
    Progress(u64, u64),
    Finished(Result<String, String>),
    FileSent(PathBuf),
    FilesPicked(Vec<PathBuf>),
    Connected(Result<(), String>),
}

struct ProgressFile {
    file: File,
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
            let _ = self.events.send(Event::Progress(self.sent, self.total));
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

fn check_receiver(settings: &Settings) -> Result<(), String> {
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
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("Android повернув HTTP {}. Перевір код і версію Android Synchro.",
            response.status()))
    }
}

fn send_files(files: Vec<PathBuf>, settings: Settings, tx: Sender<Event>) -> Result<String, String> {
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
        let file = File::open(&path).map_err(|e| e.to_string())?;
        let reader = ProgressFile {
            file, sent: 0, total: metadata.len(), events: tx.clone(), last_report: 0,
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
        let _ = tx.send(Event::FileSent(path.clone()));
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
}

impl Default for SynchroApp {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            settings: load_settings(), files: vec![], manual_path: String::new(),
            status: "Pair Android, then drop APK/ZIP files here".into(),
            progress: 0.0, busy: false, tx, rx,
            page: ui::Page::default(), connection: ui::Connection::default(),
            checking: false, last_checked_pair: None,
            show_token: false, show_manual_path: false,
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
            Ok(()) => self.status = "Налаштування збережено.".into(),
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
        self.checking = true;
        self.connection = ui::Connection::Unknown;
        self.last_checked_pair = Some((self.settings.address.clone(), self.settings.token.clone()));
        let settings = Settings {
            address: self.settings.address.clone(),
            token: self.settings.token.clone(),
            auto_send: self.settings.auto_send,
        };
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = check_receiver(&settings);
            let _ = tx.send(Event::Connected(result));
        });
    }

    fn start(&mut self) {
        if self.busy || self.files.is_empty() { return; }
        if let Err(e) = target_url(&self.settings.address) {
            self.status = e;
            return;
        }
        if let Err(e) = save_settings(&self.settings) {
            self.status = format!("Can't save pairing: {e}");
            return;
        }
        self.busy = true;
        self.progress = 0.0;
        let files = std::mem::take(&mut self.files);
        let settings = Settings {
            address: self.settings.address.clone(), token: self.settings.token.clone(),
            auto_send: self.settings.auto_send,
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
        let mut new_files = false;
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Status(s) => { self.status = s; self.progress = 0.0; }
                Event::Progress(done, total) => {
                    if total > 0 { self.progress = done as f32 / total as f32; }
                }
                Event::Finished(result) => {
                    self.busy = false;
                    self.status = result.unwrap_or_else(|e| format!("Помилка передачі: {e}"));
                }
                Event::FileSent(path) => {
                    self.files.retain(|f| f != &path);
                }
                Event::FilesPicked(files) => {
                    for file in files {
                        self.add_path(file);
                        new_files = true;
                    }
                }
                Event::Connected(result) => {
                    self.checking = false;
                    self.connection = match result {
                        Ok(()) => ui::Connection::Online,
                        Err(e) => ui::Connection::Offline(e),
                    };
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
        // Render the real transfer state and controls; no simulated phones or QR codes.
        ui::render(ui, self);
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
}
