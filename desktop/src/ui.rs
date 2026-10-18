//! Synchro desktop UI: real state, no placeholder devices or fake QR codes.
use super::*;
use egui::{Color32, RichText, Stroke};

fn c(r: u8, g: u8, b: u8) -> Color32 { Color32::from_rgb(r, g, b) }
const BLUE: Color32 = Color32::from_rgb(40, 127, 255);
const LIGHT: Color32 = Color32::from_rgb(235, 241, 255);
const MUTED: Color32 = Color32::from_rgb(144, 165, 195);
const PANEL_BG: Color32 = Color32::from_rgb(17, 27, 45);
const PANEL_BORDER: Color32 = Color32::from_rgb(42, 63, 92);
const GREEN: Color32 = Color32::from_rgb(46, 221, 122);

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Transfer,
    Devices,
    Settings,
    About,
}

#[derive(Default, Clone)]
pub enum Connection {
    #[default]
    Unknown,
    Online,
    Offline(String),
}

fn caption(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(RichText::new(text.into()).color(MUTED).size(13.0));
}

fn title(ui: &mut egui::Ui, text: impl Into<String>, size: f32) {
    ui.label(RichText::new(text.into()).color(LIGHT).size(size).strong());
}

fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new().fill(PANEL_BG)
        .stroke(Stroke::new(1.0, PANEL_BORDER))
        .corner_radius(16).inner_margin(19)
        .show(ui, add);
}

fn solid_button(text: impl Into<String>, primary: bool, width: f32) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.into()).color(LIGHT).size(15.0).strong())
        .fill(if primary { BLUE } else { c(35, 50, 76) })
        .corner_radius(10).min_size(egui::vec2(width, 40.0))
}

fn nav_button(ui: &mut egui::Ui, text: &str, selected: bool) -> bool {
    let button = egui::Button::new(RichText::new(text).color(if selected { LIGHT } else { MUTED }).size(16.0))
        .fill(if selected { c(26, 64, 121) } else { Color32::TRANSPARENT })
        .corner_radius(11).min_size(egui::vec2(185.0, 44.0));
    ui.add(button).clicked()
}

fn status_text(connection: &Connection) -> (&'static str, Color32) {
    match connection {
        Connection::Online => ("Android онлайн", GREEN),
        Connection::Offline(_) => ("Немає зв'язку", c(249, 131, 107)),
        Connection::Unknown => ("Не перевірено", MUTED),
    }
}

fn sidebar(ui: &mut egui::Ui, app: &mut SynchroApp) {
    ui.add_space(22.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("ϟ").size(44.0).color(BLUE).strong());
        ui.vertical(|ui| {
            title(ui, "Synchro", 24.0);
            caption(ui, "v0.2.0 · Rust + Android");
        });
    });
    ui.add_space(35.0);
    if nav_button(ui, "➤   Передача", app.page == Page::Transfer) { app.page = Page::Transfer; }
    ui.add_space(5.0);
    if nav_button(ui, "▣   Пристрій", app.page == Page::Devices) { app.page = Page::Devices; }
    ui.add_space(5.0);
    if nav_button(ui, "⚙   Налаштування", app.page == Page::Settings) { app.page = Page::Settings; }
    ui.add_space(5.0);
    if nav_button(ui, "ⓘ   Про програму", app.page == Page::About) { app.page = Page::About; }
    ui.add_space(45.0);
    let (state, color) = status_text(&app.connection);
    ui.horizontal(|ui| {
        ui.label(RichText::new("●").color(color).size(17.0));
        ui.label(RichText::new(state).color(MUTED).size(13.0));
    });
}

fn hero(ui: &mut egui::Ui, app: &SynchroApp) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            title(ui, "Швидка передача файлів", 30.0);
            ui.label(RichText::new("з ПК на Android").color(c(66, 153, 255)).size(29.0).strong());
            ui.add_space(6.0);
            caption(ui, "APK, ZIP, документи — напряму через Wi-Fi або Tailscale.");
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new("▣  →  ◈").color(BLUE).size(38.0));
        });
    });
    ui.add_space(12.0);
    let (state, color) = status_text(&app.connection);
    ui.horizontal(|ui| {
        ui.label(RichText::new("●").color(color).size(17.0));
        ui.label(RichText::new(state).color(color).size(14.0).strong());
        ui.separator();
        caption(ui, "Пряме з'єднання без Viber і архівування");
    });
}

fn device_card(ui: &mut egui::Ui, app: &mut SynchroApp) {
    card(ui, |ui| {
        ui.horizontal(|ui| {
            title(ui, "▣  Android", 19.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if app.checking { "Перевіряємо…" } else { "↻ Перевірити" };
                if ui.add_enabled(!app.checking, solid_button(label, false, 128.0)).clicked() {
                    app.check_connection();
                }
            });
        });
        ui.add_space(7.0);
        caption(ui, "Адреса приймача");
        let address_width = ui.available_width();
        ui.add(egui::TextEdit::singleline(&mut app.settings.address)
            .hint_text("http://192.168.1.100:8765")
            .desired_width(address_width));
        ui.add_space(7.0);
        caption(ui, "Код сполучення (32 символи)");
        ui.horizontal(|ui| {
            let token_width = (ui.available_width() - 90.0).max(120.0);
            ui.add(egui::TextEdit::singleline(&mut app.settings.token)
                .password(!app.show_token).desired_width(token_width)
                .hint_text("Встав код із Synchro Android"));
            if ui.button(if app.show_token { "Сховати" } else { "Показати" }).clicked() {
                app.show_token = !app.show_token;
            }
        });
        ui.add_space(5.0);
        ui.horizontal(|ui| {
            if ui.small_button("Зберегти").clicked() { app.persist_settings(); }
            if ui.small_button("Інструкція").clicked() { app.page = Page::Devices; }
        });
        if let Connection::Offline(reason) = &app.connection {
            ui.add_space(5.0);
            ui.label(RichText::new(reason).color(c(249, 131, 107)).size(12.0));
        }
    });
}

fn drop_card(ui: &mut egui::Ui, app: &mut SynchroApp) {
    card(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(9.0);
            ui.label(RichText::new("⇧").color(BLUE).size(45.0));
            title(ui, "Перетягни файли сюди", 20.0);
            caption(ui, "APK, ZIP та будь-які інші файли · до 2 ГіБ на файл");
            ui.add_space(12.0);
            if ui.add(solid_button("▱  Вибрати файли", false, 174.0)).clicked() {
                app.choose_files();
            }
            ui.add_space(8.0);
        });
    });
}

fn queue_card(ui: &mut egui::Ui, app: &mut SynchroApp) {
    card(ui, |ui| {
        ui.horizontal(|ui| {
            title(ui, format!("Черга файлів  ·  {}", app.files.len()), 17.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(!app.busy && !app.files.is_empty(),
                    egui::Button::new("Очистити").frame(false)).clicked() {
                    app.files.clear();
                }
            });
        });
        ui.add_space(4.0);
        if app.files.is_empty() {
            caption(ui, "Черга порожня");
            caption(ui, "Перетягни APK у вікно або натисни «Вибрати файли».");
            ui.add_space(8.0);
        } else {
            let mut remove = None;
            egui::ScrollArea::vertical().max_height(165.0).show(ui, |ui| {
                for (i, file) in app.files.iter().enumerate() {
                    let name = file.file_name().map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| file.display().to_string());
                    let bytes = file.metadata().map(|m| m.len()).unwrap_or(0);
                    ui.horizontal(|ui| {
                        let glyph = if name.to_ascii_lowercase().ends_with(".apk") { "◆" } else { "▤" };
                        ui.label(RichText::new(glyph).color(BLUE).size(23.0));
                        ui.vertical(|ui| {
                            ui.label(RichText::new(name).color(LIGHT).size(13.0));
                            caption(ui, file_size(bytes));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add_enabled(!app.busy,
                                egui::Button::new("✕").frame(false)).clicked() { remove = Some(i); }
                        });
                    });
                    ui.separator();
                }
            });
            if let Some(index) = remove { app.files.remove(index); }
        }
    });
}

fn file_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 { format!("{:.1} МБ", bytes as f64 / 1_048_576.0) }
    else if bytes >= 1024 { format!("{:.0} КБ", bytes as f64 / 1024.0) }
    else { format!("{bytes} Б") }
}

fn transfer(ui: &mut egui::Ui, app: &mut SynchroApp) {
    hero(ui, app);
    ui.add_space(18.0);
    // The panels stack on narrow windows instead of overflowing.
    if ui.available_width() >= 790.0 {
        ui.columns(2, |cols| {
            device_card(&mut cols[0], app);
            card(&mut cols[1], |ui| {
                title(ui, "Підключення", 19.0);
                ui.add_space(9.0);
                caption(ui, "1  Відкрий Synchro на Android");
                caption(ui, "2  Натисни «Start receiving»");
                caption(ui, "3  Встав IP-адресу й код з телефона");
                caption(ui, "4  Перевір з'єднання та відправ APK");
                ui.add_space(6.0);
                caption(ui, "Папка: Download/Synchro/Debug");
                ui.add_space(6.0);
                ui.label(RichText::new("QR-сполучення · у розробці").color(MUTED).size(12.0));
            });
        });
    } else {
        device_card(ui, app);
    }
    ui.add_space(12.0);
    if ui.available_width() >= 790.0 {
        ui.columns(2, |cols| {
            drop_card(&mut cols[0], app);
            queue_card(&mut cols[1], app);
        });
    } else {
        drop_card(ui, app);
        ui.add_space(10.0);
        queue_card(ui, app);
    }
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        let send_width = (ui.available_width() * 0.52).max(220.0);
        if ui.add_enabled(!app.busy && !app.files.is_empty(),
            solid_button(format!("➤  Відправити зараз  ({})", app.files.len()), true,
                send_width)).clicked() {
            app.start();
        }
        if ui.add(solid_button("＋  Додати шлях", false, 162.0)).clicked() {
            app.show_manual_path = !app.show_manual_path;
        }
    });
    if app.show_manual_path {
        ui.horizontal(|ui| {
            let path_width = (ui.available_width() - 95.0).max(160.0);
            ui.add(egui::TextEdit::singleline(&mut app.manual_path)
                .desired_width(path_width)
                .hint_text(r"C:\builds\app-debug.apk"));
            if ui.button("Додати").clicked() {
                let path = PathBuf::from(app.manual_path.trim());
                app.add_path(path);
            }
        });
    }
    if app.busy {
        ui.add_space(5.0);
        let progress_width = ui.available_width();
        ui.add(egui::ProgressBar::new(app.progress).show_percentage()
            .fill(BLUE).desired_width(progress_width));
    }
    ui.add_space(6.0);
    caption(ui, format!("Статус: {}", app.status));
    ui.add_space(4.0);
    caption(ui, "APK встановлюється тільки після підтвердження Android.");
}

fn devices(ui: &mut egui::Ui, app: &mut SynchroApp) {
    title(ui, "Підключити Android", 29.0);
    caption(ui, "Wi-Fi для локальної передачі або Tailscale для віддаленої.");
    ui.add_space(18.0);
    device_card(ui, app);
    ui.add_space(16.0);
    card(ui, |ui| {
        title(ui, "Як підключитися", 18.0);
        ui.add_space(10.0);
        for step in [
            "1. Встанови Synchro на телефон і відкрий застосунок.",
            "2. Натисни «Start receiving» (дозволь сповіщення, якщо потрібно).",
            "3. Скопіюй адресу http://IP:8765 та секретний код із телефона.",
            "4. Введи їх угорі й натисни «Перевірити».",
            "5. Повернися до передачі, перетягни APK та відправ.",
        ] { caption(ui, step); }
        ui.add_space(14.0);
        caption(ui, "Не відкривай порт 8765 в інтернет. Для віддаленої роботи використовуй Tailscale.");
    });
}

fn settings_page(ui: &mut egui::Ui, app: &mut SynchroApp) {
    title(ui, "Налаштування", 30.0);
    ui.add_space(20.0);
    card(ui, |ui| {
        title(ui, "Передача", 18.0);
        ui.add_space(10.0);
        ui.checkbox(&mut app.settings.auto_send, "Автоматично відправляти файли після перетягування");
        caption(ui, "Вимкни для ручного підтвердження кожної відправки.");
        ui.add_space(15.0);
        if ui.add(solid_button("Зберегти налаштування", true, 220.0)).clicked() {
            app.persist_settings();
        }
    });
    ui.add_space(12.0);
    card(ui, |ui| {
        title(ui, "Безпека", 18.0);
        caption(ui, "Секретний код сполучення зберігається у профілі Windows.");
        caption(ui, "У довіреній локальній мережі використовується HTTP; через інтернет — тільки Tailscale.");
        caption(ui, "Не передавай секретний код іншим людям.");
    });
}

fn about(ui: &mut egui::Ui) {
    title(ui, "Synchro", 32.0);
    caption(ui, "v0.2.0 · Open source · MIT");
    ui.add_space(18.0);
    card(ui, |ui| {
        title(ui, "Швидше, ніж пересилати через месенджер", 18.0);
        caption(ui, "Rust + egui · Windows sender");
        caption(ui, "Kotlin · Android receiver");
        caption(ui, "Протокол: SHA-256 + секретний код + прямий HTTP через Wi-Fi/Tailscale");
        ui.add_space(9.0);
        ui.hyperlink_to("github.com/kabasik007/synchro", "https://github.com/kabasik007/synchro");
    });
}

pub fn configure_theme(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = c(10, 18, 34);
    visuals.window_fill = c(10, 18, 34);
    visuals.extreme_bg_color = c(12, 21, 38);
    visuals.widgets.inactive.bg_fill = c(28, 41, 64);
    visuals.widgets.hovered.bg_fill = c(42, 69, 107);
    visuals.widgets.active.bg_fill = BLUE;
    visuals.widgets.noninteractive.fg_stroke.color = MUTED;
    ctx.set_visuals(visuals);
    ctx.global_style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(9.0, 9.0);
        style.spacing.button_padding = egui::vec2(11.0, 7.0);
    });
}

pub fn render(ui: &mut egui::Ui, app: &mut SynchroApp) {
    egui::Panel::left("synchro-sidebar").exact_size(229.0).resizable(false)
        .frame(egui::Frame::new().fill(c(11, 19, 34)).inner_margin(20)
            .stroke(Stroke::new(1.0, PANEL_BORDER)))
        .show(ui, |ui| sidebar(ui, app));
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(c(10, 18, 34)).inner_margin(24))
        .show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                match app.page {
                    Page::Transfer => transfer(ui, app),
                    Page::Devices => devices(ui, app),
                    Page::Settings => settings_page(ui, app),
                    Page::About => about(ui),
                }
            });
        });
}
