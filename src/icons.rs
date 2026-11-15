// Иконки Material Symbols (приватная область юникода) + установка шрифта в egui.
use egui::{FontData, FontDefinitions, FontFamily, FontId};

pub const MATERIAL_FAMILY: &str = "material";
pub const FONT_BYTES: &[u8] = include_bytes!("../assets/MaterialSymbolsOutlined.ttf");

pub fn material_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MATERIAL_FAMILY.into()))
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .insert(MATERIAL_FAMILY.to_owned(), FontData::from_static(FONT_BYTES));
    fonts
        .families
        .insert(FontFamily::Name(MATERIAL_FAMILY.into()), vec![MATERIAL_FAMILY.to_owned()]);
    ctx.set_fonts(fonts);
}

/// Имя иконки -> глиф. Все кодпоинты сверены с официальным codepoints-файлом Material Symbols.
pub fn icon(name: &str) -> &'static str {
    match name {
        "cast" => "\u{e307}",
        "home" => "\u{e9b2}",
        "arrow_back" => "\u{e5c4}",
        "apps" => "\u{e5c3}",
        "settings" => "\u{e8b8}",
        "volume_up" => "\u{e050}",
        "volume_down" => "\u{e04d}",
        "power" => "\u{f8c7}",
        "photo_camera" => "\u{e412}",
        "screen_rotation" => "\u{f2d5}",
        "keyboard" => "\u{e312}",
        "smartphone" => "\u{e7ba}",
        "fullscreen" => "\u{e5d0}",
        "refresh" => "\u{e5d5}",
        "touch_app" => "\u{e913}",
        "palette" => "\u{e40a}",
        "link" => "\u{e250}",
        "devices" => "\u{e326}",
        "play_arrow" => "\u{e037}",
        "close" => "\u{e5cd}",
        "notifications" => "\u{e7f5}",
        "send" => "\u{e163}",
        "volume_off" => "\u{e04f}",
        "volume_mute" => "\u{e04e}",
        "screen_lock_rotation" => "\u{f2d6}",
        "chevron_right" => "\u{e5cc}",
        "more_vert" => "\u{e5d4}",
        "vibration" => "\u{f2cb}",
        _ => "?",
    }
}
