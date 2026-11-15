// Material Design 3: цветовые роли, тема egui и переиспользуемые виджеты.
use egui::{
    Button, Color32, Context, Frame, Response, RichText, Rounding, Sense, Stroke, TextStyle, Ui,
    Vec2,
};
use crate::icons;

#[derive(Clone, Copy)]
pub struct Md3 {
    pub surface: Color32,
    pub surface_container_lowest: Color32,
    pub surface_container_low: Color32,
    pub surface_container: Color32,
    pub surface_container_high: Color32,
    pub surface_container_highest: Color32,
    pub on_surface: Color32,
    pub on_surface_variant: Color32,
    pub primary: Color32,
    pub on_primary: Color32,
    pub primary_container: Color32,
    pub on_primary_container: Color32,
    pub secondary_container: Color32,
    pub on_secondary_container: Color32,
    pub outline: Color32,
    pub outline_variant: Color32,
    pub error: Color32,
    pub error_container: Color32,
    pub success: Color32,
    pub dark: bool,
}

fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

impl Md3 {
    pub fn dark() -> Self {
        Self {
            surface: rgb(0x141218),
            surface_container_lowest: rgb(0x0F0D13),
            surface_container_low: rgb(0x1D1B20),
            surface_container: rgb(0x211F26),
            surface_container_high: rgb(0x2B2930),
            surface_container_highest: rgb(0x36343B),
            on_surface: rgb(0xE6E0E9),
            on_surface_variant: rgb(0xCAC4D0),
            primary: rgb(0xD0BCFF),
            on_primary: rgb(0x381E72),
            primary_container: rgb(0x4F378B),
            on_primary_container: rgb(0xEADDFF),
            secondary_container: rgb(0x4A4458),
            on_secondary_container: rgb(0xE8DEF8),
            outline: rgb(0x938F99),
            outline_variant: rgb(0x49454F),
            error: rgb(0xF2B8B5),
            error_container: rgb(0x8C1D18),
            success: rgb(0x8FD49C),
            dark: true,
        }
    }

    pub fn light() -> Self {
        Self {
            surface: rgb(0xFEF7FF),
            surface_container_lowest: rgb(0xFFFFFF),
            surface_container_low: rgb(0xF7F2FA),
            surface_container: rgb(0xF3EDF7),
            surface_container_high: rgb(0xECE6F0),
            surface_container_highest: rgb(0xE6E0E9),
            on_surface: rgb(0x1D1B20),
            on_surface_variant: rgb(0x49454F),
            primary: rgb(0x6750A4),
            on_primary: rgb(0xFFFFFF),
            primary_container: rgb(0xEADDFF),
            on_primary_container: rgb(0x21005D),
            secondary_container: rgb(0xE8DEF8),
            on_secondary_container: rgb(0x1D192B),
            outline: rgb(0x79747E),
            outline_variant: rgb(0xCAC4D0),
            error: rgb(0xB3261E),
            error_container: rgb(0xF9DEDC),
            success: rgb(0x2E6B3F),
            dark: false,
        }
    }
}

/// Применить тему MD3 к egui (визуалы, отступы, размеры шрифтов).
pub fn apply(ctx: &Context, c: &Md3) {
    let mut style = (*ctx.style()).clone();
    let v = &mut style.visuals;
    v.dark_mode = c.dark;
    v.override_text_color = Some(c.on_surface);
    v.window_fill = c.surface;
    v.panel_fill = c.surface_container_low;
    v.extreme_bg_color = c.surface_container_lowest;
    v.faint_bg_color = c.surface_container;
    v.window_stroke = Stroke::NONE;
    v.window_rounding = Rounding::same(16.0);
    v.selection.bg_fill = c.primary_container;
    v.selection.stroke = Stroke::new(1.0, c.primary);
    v.hyperlink_color = c.primary;

    let rounding = Rounding::same(10.0);
    v.widgets.noninteractive.bg_fill = c.surface_container_low;
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, c.on_surface);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, c.outline_variant);
    v.widgets.noninteractive.rounding = rounding;
    v.widgets.inactive.bg_fill = Color32::TRANSPARENT;
    v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, c.on_surface_variant);
    v.widgets.inactive.rounding = rounding;
    v.widgets.hovered.bg_fill = c.surface_container_high;
    v.widgets.hovered.weak_bg_fill = c.surface_container_high;
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, c.on_surface);
    v.widgets.hovered.rounding = rounding;
    v.widgets.active.bg_fill = c.surface_container_highest;
    v.widgets.active.weak_bg_fill = c.surface_container_highest;
    v.widgets.active.fg_stroke = Stroke::new(1.0, c.on_surface);
    v.widgets.active.rounding = rounding;
    v.widgets.open.bg_fill = c.surface_container_high;
    v.widgets.open.weak_bg_fill = c.surface_container_high;

    style.spacing.button_padding = Vec2::new(18.0, 9.0);
    style.spacing.item_spacing = Vec2::new(10.0, 10.0);
    style.spacing.icon_width = 24.0;
    style.spacing.icon_width_inner = 18.0;

    let mut ts = style.text_styles.clone();
    ts.insert(TextStyle::Heading, egui::FontId::proportional(22.0));
    ts.insert(TextStyle::Body, egui::FontId::proportional(15.0));
    ts.insert(TextStyle::Button, egui::FontId::proportional(15.0));
    ts.insert(TextStyle::Small, egui::FontId::proportional(12.0));
    ts.insert(TextStyle::Monospace, egui::FontId::monospace(12.0));
    style.text_styles = ts;

    ctx.set_style(style);
}

// ---------- виджеты ----------

/// Filled-кнопка (заливка primary, pill-форма) — главное действие.
pub fn filled_button(ui: &mut Ui, label: &str) -> Response {
    let c = palette(ui);
    ui.add(
        Button::new(RichText::new(label).color(c.on_primary))
            .fill(c.primary)
            .rounding(Rounding::same(24.0)),
    )
}

/// Tonal-кнопка (secondary_container) — второстепенное действие.
pub fn tonal_button(ui: &mut Ui, label: &str) -> Response {
    let c = palette(ui);
    ui.add(
        Button::new(RichText::new(label).color(c.on_secondary_container))
            .fill(c.secondary_container)
            .rounding(Rounding::same(24.0)),
    )
}

/// Outlined-кнопка.
pub fn outlined_button(ui: &mut Ui, label: &str) -> Response {
    let c = palette(ui);
    ui.add(
        Button::new(RichText::new(label).color(c.primary))
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::new(1.0, c.outline))
            .rounding(Rounding::same(24.0)),
    )
}

/// Квадратная иконка-кнопка 40x40.
pub fn icon_button(ui: &mut Ui, name: &str, tooltip: &str) -> Response {
    let c = palette(ui);
    let resp = ui.add(
        Button::new(RichText::new(icons::icon(name)).font(icons::material_font(20.0)).color(c.on_surface_variant))
            .fill(Color32::TRANSPARENT)
            .min_size(Vec2::new(40.0, 40.0))
            .rounding(Rounding::same(12.0)),
    );
    resp.on_hover_text(tooltip)
}

/// Иконка-кнопка с цветом.
pub fn icon_button_colored(ui: &mut Ui, name: &str, tooltip: &str, color: Color32) -> Response {
    let c = palette(ui);
    let resp = ui.add(
        Button::new(RichText::new(icons::icon(name)).font(icons::material_font(20.0)).color(color))
            .fill(Color32::TRANSPARENT)
            .min_size(Vec2::new(40.0, 40.0))
            .rounding(Rounding::same(12.0)),
    );
    resp.on_hover_text(tooltip)
}

/// FAB (floating action button) — большая круглая кнопка.
pub fn fab(ui: &mut Ui, name: &str, tooltip: &str) -> Response {
    let c = palette(ui);
    let resp = ui.add(
        Button::new(RichText::new(icons::icon(name)).font(icons::material_font(26.0)).color(c.on_primary_container))
            .fill(c.primary_container)
            .min_size(Vec2::new(56.0, 56.0))
            .rounding(Rounding::same(28.0)),
    );
    resp.on_hover_text(tooltip)
}

/// Chip — маленькая «таблетка» с текстом.
pub fn chip(ui: &mut Ui, text: &str, fill: Color32, fg: Color32) {
    Frame::none()
        .fill(fill)
        .rounding(Rounding::same(10.0))
        .inner_margin(egui::Margin::symmetric(10.0, 4.0))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(12.0).color(fg));
        });
}

/// Карточка (surface container, скругление 16).
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let c = palette(ui);
    Frame::none()
        .fill(c.surface_container)
        .rounding(Rounding::same(16.0))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, add)
        .inner
}

/// Карточка с произвольной заливкой.
pub fn card_fill<R>(ui: &mut Ui, fill: Color32, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::none()
        .fill(fill)
        .rounding(Rounding::same(16.0))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, add)
        .inner
}

/// Заголовок секции (label-large, on_surface_variant).
pub fn section_title(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(13.0).color(palette(ui).on_surface_variant).strong());
}

/// Заголовок карточки с ведущей иконкой.
pub fn card_title(ui: &mut Ui, icon_name: &str, text: &str) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(icons::icon(icon_name))
                .font(icons::material_font(20.0))
                .color(palette(ui).primary),
        );
        ui.label(RichText::new(text).size(16.0).strong());
    });
    ui.add_space(6.0);
}

/// Пустое состояние: большая иконка + заголовок + подпись.
pub fn empty_state(ui: &mut Ui, icon_name: &str, title: &str, subtitle: &str) {
    let c = palette(ui);
    ui.vertical_centered(|ui| {
        ui.add_space(24.0);
        ui.label(
            RichText::new(icons::icon(icon_name))
                .font(icons::material_font(56.0))
                .color(c.on_surface_variant),
        );
        ui.add_space(8.0);
        ui.label(RichText::new(title).size(18.0).strong());
        ui.label(RichText::new(subtitle).size(13.0).color(c.on_surface_variant));
        ui.add_space(16.0);
    });
}

/// MD3-переключатель: строка с текстом, подписью и toggle справа (рисуем вручную).
pub fn switch_row(ui: &mut Ui, text: &str, subtitle: &str, on: &mut bool) -> Response {
    let c = palette(ui);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 44.0), Sense::click());
    ui.painter().text(
        egui::pos2(rect.left() + 4.0, rect.center().y - 7.0),
        egui::Align2::LEFT_CENTER,
        text,
        egui::FontId::proportional(15.0),
        c.on_surface,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 4.0, rect.center().y + 10.0),
        egui::Align2::LEFT_CENTER,
        subtitle,
        egui::FontId::proportional(12.0),
        c.on_surface_variant,
    );
    // дорожка и бегунок
    let track_w = 44.0;
    let track_h = 26.0;
    let track = egui::Rect::from_center_size(
        egui::pos2(rect.right() - track_w / 2.0 - 4.0, rect.center().y),
        Vec2::new(track_w, track_h),
    );
    let (fill, thumb_fill) = if *on {
        (c.primary, c.on_primary)
    } else {
        (c.surface_container_highest, c.outline)
    };
    ui.painter().rect_filled(track, Rounding::same(track_h / 2.0), fill);
    let cx = if *on { track.right() - 11.0 } else { track.left() + 11.0 };
    ui.painter().circle_filled(egui::pos2(cx, track.center().y), 9.0, thumb_fill);
    if resp.clicked() {
        *on = !*on;
    }
    resp
}

/// Вертикальный Navigation Rail элемент.
pub fn rail_item(ui: &mut Ui, icon_name: &str, label: &str, selected: bool) -> Response {
    let c = palette(ui);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(72.0, 56.0), Sense::click());
    let pill = egui::Rect::from_center_size(rect.center() - Vec2::new(0.0, 7.0), Vec2::new(44.0, 30.0));
    let fill = if selected {
        c.secondary_container
    } else if resp.hovered() {
        c.surface_container_high
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(pill, Rounding::same(15.0), fill);
    ui.painter().text(
        pill.center(),
        egui::Align2::CENTER_CENTER,
        icons::icon(icon_name),
        icons::material_font(20.0),
        if selected { c.on_secondary_container } else { c.on_surface_variant },
    );
    ui.painter().text(
        rect.center() + Vec2::new(0.0, 18.0),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(11.0),
        if selected { c.primary } else { c.on_surface_variant },
    );
    resp
}

/// Доступ к текущей теме из любого места UI (палитра кладётся в ctx.data).
pub fn palette(ui: &Ui) -> Md3 {
    ui.ctx()
        .data_mut(|d| {
            d.get_temp::<Md3>(egui::Id::new("md3_colors"))
                .unwrap_or(if ui.visuals().dark_mode { Md3::dark() } else { Md3::light() })
        })
}

/// Сохранить текущую палитру для виджетов.
pub fn set_colors(ctx: &Context, c: &Md3) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("md3_colors"), *c));
}
