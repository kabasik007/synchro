//! Shared Synchro Desktop visual system derived from docs/design/README.md.
//! No UI asset downloader or browser runtime: standard Rust/egui primitives.
use eframe::egui::{self, Color32, RichText, Stroke};

pub const BACKGROUND: Color32 = Color32::from_rgb(9, 17, 32);
pub const SIDEBAR: Color32 = Color32::from_rgb(11, 21, 39);
pub const CARD: Color32 = Color32::from_rgb(17, 30, 51);
pub const CARD_ALT: Color32 = Color32::from_rgb(19, 39, 69);
pub const BORDER: Color32 = Color32::from_rgb(42, 63, 93);
pub const BLUE: Color32 = Color32::from_rgb(54, 139, 255);
pub const PALE_BLUE: Color32 = Color32::from_rgb(119, 185, 255);
pub const TEXT: Color32 = Color32::from_rgb(235, 242, 255);
pub const MUTED: Color32 = Color32::from_rgb(150, 171, 200);
pub const GREEN: Color32 = Color32::from_rgb(62, 216, 145);
pub const ORANGE: Color32 = Color32::from_rgb(247, 173, 101);
pub const RED: Color32 = Color32::from_rgb(249, 131, 107);

pub fn is_internet(address: &str) -> bool {
    address.trim().starts_with("https://")
}

pub fn transport_name(address: &str) -> &'static str {
    if is_internet(address) { "INTERNET MODE" } else { "LAN / TAILSCALE" }
}

pub fn eyebrow(ui: &mut egui::Ui, label: &str) {
    ui.label(RichText::new(label).color(PALE_BLUE).size(11.0).strong());
}

pub fn chip(ui: &mut egui::Ui, label: &str, dot: Color32) {
    egui::Frame::new()
        .fill(Color32::from_rgb(27, 45, 72))
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(10)
        .inner_margin(egui::Margin::symmetric(10, 5))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(RichText::new("●").color(dot).size(10.0));
                ui.label(RichText::new(label).color(TEXT).size(12.0));
            });
        });
}

/// A small custom vector illustration matching the approved PC -> network -> phone
/// concept. This is decoration only, never an invented connection indicator.
pub fn connection_art(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(202.0, 99.0), egui::Sense::hover());
    let painter = ui.painter();
    let origin = rect.min;
    let screen = egui::Rect::from_min_size(origin + egui::vec2(13.0, 19.0), egui::vec2(67.0, 48.0));
    painter.rect_filled(screen, 7, Color32::from_rgb(67, 102, 153));
    painter.rect_filled(screen.shrink(3.0), 5, Color32::from_rgb(15, 35, 63));
    painter.rect_filled(
        egui::Rect::from_min_size(origin + egui::vec2(43.0, 67.0), egui::vec2(7.0, 10.0)),
        1, Color32::from_rgb(108, 156, 221)
    );
    painter.line_segment(
        [origin + egui::vec2(29.0, 78.0), origin + egui::vec2(64.0, 78.0)],
        Stroke::new(3.0, Color32::from_rgb(108, 156, 221))
    );
    painter.line_segment(
        [origin + egui::vec2(87.0, 45.0), origin + egui::vec2(117.0, 45.0)],
        Stroke::new(2.0, PALE_BLUE)
    );
    painter.circle_filled(origin + egui::vec2(103.0, 45.0), 4.0, BLUE);
    painter.circle_stroke(origin + egui::vec2(103.0, 45.0), 11.0, Stroke::new(1.0, BLUE.gamma_multiply(0.42)));
    let phone = egui::Rect::from_min_size(origin + egui::vec2(128.0, 12.0), egui::vec2(44.0, 73.0));
    painter.rect_filled(phone, 10, Color32::from_rgb(77, 119, 179));
    painter.rect_filled(phone.shrink(3.0), 7, Color32::from_rgb(17, 34, 60));
    painter.circle_filled(origin + egui::vec2(150.0, 78.0), 2.0, PALE_BLUE);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transport_badge_reflects_the_configured_mode() {
        assert_eq!(transport_name("https://disk.sitezilla.net"), "INTERNET MODE");
        assert_eq!(transport_name("http://192.168.1.20:8765"), "LAN / TAILSCALE");
        assert_eq!(transport_name(""), "LAN / TAILSCALE");
    }
}
