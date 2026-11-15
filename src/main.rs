// Android Mirror — минимальный режим: безрамочное окно = экран телефона,
// управление мышью как тачем, кнопки — в системном трее.
// Автоподключение при запуске; сервер scrcpy v2.7 (пакетный режим).
mod adb;
mod connection;
mod control;
mod icons;
mod keyboard;
mod md3;

use adb::{Adb, Device, ServerParams};
use connection::{Connection, Shared, Status};
use control::ControlMsg;
use egui::{
    vec2, Color32, ColorImage, Frame, Pos2, Rect, RichText, Rounding, Sense, TextureHandle,
    TextureOptions, Ui, Vec2,
};
use md3::Md3;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::TrayIconEvent;

// коды Android-клавиш
const KEY_BACK: u32 = 4;
const KEY_HOME: u32 = 3;
const KEY_VOLUME_UP: u32 = 24;
const KEY_VOLUME_DOWN: u32 = 25;
const KEY_RECENTS: u32 = 187;

#[derive(Clone, Copy)]
struct Settings {
    bitrate_mbps: u32,
    max_fps: u32,  // 0 = без ограничения
    max_size: u32, // 0 = нативное разрешение
    show_touches: bool,
    stay_awake: bool,
    virtual_display: bool, // scrcpy 3.x: отдельный дисплей 1920×1080
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            bitrate_mbps: 8,
            max_fps: 0,
            max_size: 0,
            show_touches: false,
            stay_awake: true,
            virtual_display: false,
        }
    }
}

fn find_jar(name: &str) -> Option<String> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() {
            candidates.push(d.join(name));
            candidates.push(d.join(format!("../assets/{}", name)));
            candidates.push(d.join(format!("../../assets/{}", name)));
        }
    }
    candidates.push(PathBuf::from(format!("assets/{}", name)));
    candidates
        .into_iter()
        .find(|c| c.is_file())
        .map(|c| c.to_string_lossy().into_owned())
}

#[derive(Clone)]
struct TrayIds {
    reconnect: MenuId,
    disconnect: MenuId,
    home: MenuId,
    back: MenuId,
    recents: MenuId,
    vol_up: MenuId,
    vol_down: MenuId,
    screen_on: MenuId,
    screen_off: MenuId,
    shade: MenuId,
    screenshot: MenuId,
    always_on_top: MenuId,
    fullscreen: MenuId,
    keyboard: MenuId,
    settings: MenuId,
    quit: MenuId,
}

struct MirrorApp {
    adb: Option<Adb>,
    adb_path_str: String,
    devices: Vec<Device>,
    selected_serial: String,
    shared: Arc<Shared>,
    conn: Option<Arc<Connection>>,
    texture: Option<TextureHandle>,
    texture_version: u64,
    settings: Settings,
    jar_path: String,
    jar_v3_path: String,
    touch_captured: Option<(u32, u32)>,
    settings_open: bool,
    window_visible: bool,
    always_on_top: bool,
    fullscreen: bool,
    maximized: bool,
    auto_retry: bool,
    next_retry: Option<std::time::Instant>,
    tray: Option<tray_icon::TrayIcon>,
    tray_ids: TrayIds,
    tray_kb: Option<tray_icon::menu::CheckMenuItem>,
    tray_aot: Option<tray_icon::menu::CheckMenuItem>,
    keyboard_enabled: bool,
    /// меню кнопок на верхней полосе (назад/домой/громкость…)
    strip_menu: bool,
    /// активное перетаскивание: (курсор на экране при захвате, позиция окна при захвате)
    win_drag: Option<(egui::Pos2, egui::Pos2)>,
    /// нажатие в зоне полосы: (позиция, было движение)
    strip_press: Option<(egui::Pos2, bool)>,
    /// время последнего клика по полосе (для двойного клика)
    strip_last_click: Option<std::time::Instant>,
}

impl MirrorApp {
    fn new(cc: &eframe::CreationContext) -> Self {
        icons::install_fonts(&cc.egui_ctx);

        let adb = Adb::find().ok();
        let devices = adb
            .as_ref()
            .and_then(|a| a.devices().ok())
            .unwrap_or_default();
        let selected_serial = devices
            .first()
            .map(|d| d.serial.clone())
            .unwrap_or_default();
        let adb_path_str = adb
            .as_ref()
            .map(|a| a.path.to_string_lossy().into_owned())
            .unwrap_or_default();
        let shared = Shared::new();
        *shared.status.lock().unwrap() = Status::Idle;

        // размер окна: вписать телефон 9:20 в монитор, прижать к правому краю
        let monitor = cc
            .egui_ctx
            .input(|i| i.viewport().monitor_size)
            .unwrap_or(vec2(1920.0, 1080.0));
        let aspect = 1080.0 / 2400.0; // 0.45
        let h = (monitor.y * 0.9).min(940.0);
        let w = h * aspect;
        let pos = vec2((monitor.x - w - 12.0).max(0.0), (monitor.y - h) * 0.5);
        cc.egui_ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(w, h)));
        cc.egui_ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::Pos2::new(
            pos.x, pos.y,
        )));

        let tray_ids = TrayIds {
            reconnect: MenuId::new("reconnect"),
            disconnect: MenuId::new("disconnect"),
            home: MenuId::new("home"),
            back: MenuId::new("back"),
            recents: MenuId::new("recents"),
            vol_up: MenuId::new("vol_up"),
            vol_down: MenuId::new("vol_down"),
            screen_on: MenuId::new("screen_on"),
            screen_off: MenuId::new("screen_off"),
            shade: MenuId::new("shade"),
            screenshot: MenuId::new("screenshot"),
            always_on_top: MenuId::new("always_on_top"),
            fullscreen: MenuId::new("fullscreen"),
            keyboard: MenuId::new("keyboard"),
            settings: MenuId::new("settings"),
            quit: MenuId::new("quit"),
        };


        let (tray, tray_kb, tray_aot) = match Self::build_tray(&tray_ids.clone()) {
            Some((t, k, a)) => (Some(t), Some(k), Some(a)),
            None => (None, None, None),
        };
        let mut app = Self {
            adb,
            adb_path_str,
            devices,
            selected_serial,
            shared,
            conn: None,
            texture: None,
            texture_version: 0,
            settings: Settings::default(),
            jar_path: find_jar("scrcpy-server.jar")
                .unwrap_or_else(|| "assets/scrcpy-server.jar".into()),
            jar_v3_path: find_jar("scrcpy-server-v3.jar")
                .unwrap_or_else(|| "assets/scrcpy-server-v3.jar".into()),
            touch_captured: None,
            settings_open: false,
            window_visible: true,
            always_on_top: false,
            fullscreen: false,
            maximized: false,
            auto_retry: true,
            next_retry: None,
            tray,
            tray_ids,
            tray_kb,
            tray_aot,
            keyboard_enabled: true,
            strip_menu: false,
            win_drag: None,
            strip_press: None,
            strip_last_click: None,
        };
        // автоподключение при запуске
        app.connect(&cc.egui_ctx);
        app
    }

    fn build_tray(ids: &TrayIds) -> Option<(tray_icon::TrayIcon, CheckMenuItem, CheckMenuItem)> {
        let menu = Menu::new();
        let add = |menu: &Menu, id: &MenuId, label: &str| {
            menu.append(&MenuItem::with_id(id.clone(), label, true, None))
                .ok()
        };
        add(&menu, &ids.reconnect, "Переподключить");
        add(&menu, &ids.disconnect, "Отключиться");
        menu.append(&PredefinedMenuItem::separator()).ok();
        add(&menu, &ids.home, "Домой");
        add(&menu, &ids.back, "Назад");
        add(&menu, &ids.recents, "Недавние");
        menu.append(&PredefinedMenuItem::separator()).ok();
        add(&menu, &ids.vol_up, "Громче");
        add(&menu, &ids.vol_down, "Тише");
        menu.append(&PredefinedMenuItem::separator()).ok();
        add(&menu, &ids.screen_on, "Включить экран");
        add(&menu, &ids.screen_off, "Выключить экран");
        add(&menu, &ids.shade, "Шторка уведомлений");
        add(&menu, &ids.screenshot, "Скриншот телефона");
        menu.append(&PredefinedMenuItem::separator()).ok();
        let aot = CheckMenuItem::with_id(
            ids.always_on_top.clone(),
            "Поверх всех окон",
            true,
            false,
            None,
        );
        menu.append(&aot).ok();
        let kb = CheckMenuItem::with_id(
            MenuId::new("keyboard"),
            "Клавиатура ПК → телефон",
            true,
            true,
            None,
        );
        menu.append(&kb).ok();
        add(&menu, &ids.fullscreen, "Во весь экран");
        add(&menu, &ids.settings, "Настройки…");
        menu.append(&PredefinedMenuItem::separator()).ok();
        add(&menu, &ids.quit, "Выход");

        // иконка: фиолетовый скруглённый квадрат с белой «трубкой»
        let (sz, rgba) = draw_tray_icon(32);

        let icon = tray_icon::Icon::from_rgba(rgba, sz as u32, sz as u32).ok()?;
        match tray_icon::TrayIconBuilder::new()
            .with_id("android-mirror-tray")
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_tooltip("Android Mirror — ЛКМ: показать/скрыть")
            .with_icon(icon)
            .build()
        {
            Ok(t) => Some((t, kb, aot)),
            Err(e) => {
                eprintln!("tray error: {e}");
                None
            }
        }
    }

    fn colors(&self) -> Md3 {
        Md3::dark() // минимальный режим — тёмная тема
    }

    fn send_ctrl(&self, msg: ControlMsg) {
        if let Some(conn) = &self.conn {
            let _ = conn.ctrl_tx.send(msg);
        }
    }

    fn press_key(&self, keycode: u32) {
        self.send_ctrl(ControlMsg::KeyCode { action: 0, keycode, meta_state: 0, repeat: 0 });
        self.send_ctrl(ControlMsg::KeyCode { action: 1, keycode, meta_state: 0, repeat: 0 });
    }

    fn snack(&self, text: impl Into<String>) {
        self.shared.set_snack(text.into());
    }

    /// Перетаскивание окна «преследованием»: каждый кадр цель позиции окна
    /// = текущее экрашное положение курсора минус точка захвата.
    /// Интегрирование дельт здесь даёт рывки: Windows шлёт синтетический
    /// WM_MOUSEMOVE после SetWindowPos, и дельты начинают «тянуть» окно назад.
    /// Экранный курсор = позиция окна (outer_rect, свежая после WM_MOVE)
    /// + позиция указателя в окне.
    fn begin_window_drag(&mut self, ctx: &egui::Context) {
        if self.maximized || self.fullscreen {
            return;
        }
        if let Some(origin) = ctx.input(|i| i.viewport().outer_rect.map(|r| r.min)) {
            let cursor = origin + ctx.input(|i| i.pointer.latest_pos().unwrap_or_default()).to_vec2();
            self.shared.log("[drag] старт перетаскивания".into());
            self.win_drag = Some((cursor, origin));
        }
    }

    fn window_drag_frame(&mut self, ctx: &egui::Context) {
        let Some((grab_cursor, grab_origin)) = self.win_drag else {
            return;
        };
        if let Some(origin) = ctx.input(|i| i.viewport().outer_rect.map(|r| r.min)) {
            let cursor = origin + ctx.input(|i| i.pointer.latest_pos().unwrap_or_default()).to_vec2();
            let target = cursor - (grab_cursor - grab_origin);
            if (target - origin).length_sq() > 0.5 {
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(target));
            }
        }
    }

    fn end_window_drag(&mut self, ctx: &egui::Context) {
        if self.win_drag.is_some() && !ctx.input(|i| i.pointer.primary_down()) {
            self.win_drag = None;
        }
    }

    fn refresh_devices(&mut self) {
        if let Some(adb) = &self.adb {
            if let Ok(devs) = adb.devices() {
                self.devices = devs;
                if !self.devices.iter().any(|d| d.serial == self.selected_serial) {
                    self.selected_serial = self
                        .devices
                        .first()
                        .map(|d| d.serial.clone())
                        .unwrap_or_default();
                }
            }
        }
    }

    fn connect(&mut self, ctx: &egui::Context) {
        if self.conn.is_some()
            && *self.shared.status.lock().unwrap() == Status::Connecting
        {
            return;
        }
        let Some(adb) = self.adb.clone() else {
            self.snack("adb не найден");
            return;
        };
        if self.selected_serial.is_empty() {
            self.refresh_devices();
            if self.selected_serial.is_empty() {
                self.snack("Телефон не найден — проверь USB и отладку");
                return;
            }
        }
        let (jar, version, new_display) = if self.settings.virtual_display {
            let j = PathBuf::from(&self.jar_v3_path);
            if !j.is_file() {
                self.snack(format!("Не найден {}", self.jar_v3_path));
                return;
            }
            (j, connection::SCRCPY_VERSION_V3, Some((1920u32, 1080u32)))
        } else {
            let j = PathBuf::from(&self.jar_path);
            if !j.is_file() {
                self.snack(format!("Не найден {}", self.jar_path));
                return;
            }
            (j, connection::SCRCPY_VERSION, None)
        };

        self.disconnect();

        *self.shared.frame.lock().unwrap() = None;
        self.texture = None;
        self.texture_version = 0;
        *self.shared.stats.lock().unwrap() = connection::Stats::default();

        let s = self.settings;
        let params = ServerParams::new(
            version,
            new_display,
            s.bitrate_mbps * 1_000_000,
            (s.max_fps > 0).then_some(s.max_fps),
            (s.max_size > 0).then_some(s.max_size),
            s.show_touches,
            s.stay_awake,
        );
        let seq = self
            .shared
            .conn_seq
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        self.conn = Some(connection::start(
            adb,
            self.selected_serial.clone(),
            jar,
            params,
            self.shared.clone(),
            ctx.clone(),
            seq,
        ));
    }

    fn disconnect(&mut self) {
        if let Some(conn) = self.conn.take() {
            conn.shutdown();
        }
    }

    fn poll_shared(&mut self) {
        let st = self.shared.status.lock().unwrap().clone();
        if self.conn.is_some() {
            match st {
                Status::Disconnected => {
                    self.disconnect();
                    self.snack("Соединение закрыто");
                    if self.auto_retry {
                        self.next_retry =
                            Some(std::time::Instant::now() + Duration::from_secs(5));
                    }
                }
                Status::Failed(ref e) => {
                    let msg = e.clone();
                    self.disconnect();
                    self.snack(format!("Ошибка: {}", msg));
                    if self.auto_retry {
                        self.next_retry =
                            Some(std::time::Instant::now() + Duration::from_secs(5));
                    }
                }
                _ => {}
            }
        } else if st == Status::Disconnected {
            *self.shared.status.lock().unwrap() = Status::Idle;
        }
    }

    fn update_texture(&mut self, ctx: &egui::Context) {
        let frame = self.shared.frame.lock().unwrap().clone();
        if let Some(f) = frame {
            if f.version != self.texture_version {
                let img = ColorImage::from_rgba_unmultiplied([f.w, f.h], &f.rgba);
                match &mut self.texture {
                    Some(t) => t.set(img, TextureOptions::LINEAR),
                    None => {
                        self.texture =
                            Some(ctx.load_texture("phone_screen", img, TextureOptions::LINEAR))
                    }
                }
                self.texture_version = f.version;
            }
        }
    }

    fn take_screenshot(&mut self) {
        let Some(adb) = self.adb.clone() else {
            self.snack("adb не найден");
            return;
        };
        let serial = self.selected_serial.clone();
        let shared = self.shared.clone();
        let dir = PathBuf::from("screenshots");
        let _ = std::fs::create_dir_all(&dir);
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let path = dir.join(format!("phone_{}.png", secs));
        std::thread::spawn(move || match adb.screencap(&serial, &path) {
            Ok(()) => shared.set_snack(format!("Скриншот: {}", path.display())),
            Err(e) => shared.set_snack(format!("Скриншот не удался: {}", e)),
        });
    }

    fn handle_tray_events(&mut self, ctx: &egui::Context) {
        for event in MenuEvent::receiver().try_iter() {
            let id = event.id().0.clone();
            let ids = &self.tray_ids;
            if id == ids.home.0 {
                self.press_key(KEY_HOME);
            } else if id == ids.back.0 {
                self.press_key(KEY_BACK);
            } else if id == ids.recents.0 {
                self.press_key(KEY_RECENTS);
            } else if id == ids.vol_up.0 {
                self.press_key(KEY_VOLUME_UP);
            } else if id == ids.vol_down.0 {
                self.press_key(KEY_VOLUME_DOWN);
            } else if id == ids.screen_on.0 {
                self.send_ctrl(ControlMsg::ScreenPower(1));
            } else if id == ids.screen_off.0 {
                self.send_ctrl(ControlMsg::ScreenPower(0));
            } else if id == ids.shade.0 {
                self.send_ctrl(ControlMsg::ExpandNotifications);
            } else if id == ids.screenshot.0 {
                self.take_screenshot();
            } else if id == ids.reconnect.0 {
                self.auto_retry = true;
                self.refresh_devices();
                self.connect(ctx);
            } else if id == ids.disconnect.0 {
                self.auto_retry = false;
                self.disconnect();
            } else if id == ids.always_on_top.0 {
                if let Some(item) = &self.tray_aot {
                    let on = !item.is_checked();
                    item.set_checked(on);
                    self.always_on_top = on;
                    ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                        if on {
                            egui::WindowLevel::AlwaysOnTop
                        } else {
                            egui::WindowLevel::Normal
                        },
                    ));
                }
            } else if id == ids.keyboard.0 {
                if let Some(item) = &self.tray_kb {
                    item.set_checked(!item.is_checked());
                    self.keyboard_enabled = item.is_checked();
                }
            } else if id == ids.fullscreen.0 {
                self.fullscreen = !self.fullscreen;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
            } else if id == ids.settings.0 {
                self.settings_open = !self.settings_open;
                if self.settings_open {
                    self.refresh_devices();
                }
            } else if id == ids.quit.0 {
                self.disconnect();
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        // ЛКМ по иконке трея — показать/скрыть окно
        for ev in TrayIconEvent::receiver().try_iter() {
            if let TrayIconEvent::Click {
                button: tray_icon::MouseButton::Left,
                button_state: tray_icon::MouseButtonState::Up,
                ..
            } = ev
            {
                self.window_visible = !self.window_visible;
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(self.window_visible));
                if self.window_visible {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
            }
        }
    }

    fn video_area(&mut self, ui: &mut Ui, space: Rect, ctx: &egui::Context, c: &Md3) {
        let painter = ui.painter_at(space);
        painter.rect_filled(space, Rounding::same(0.0), Color32::BLACK);

        let Some(tex) = self.texture.clone() else {
            painter.text(
                space.center(),
                egui::Align2::CENTER_CENTER,
                "Нет сигнала",
                egui::FontId::proportional(16.0),
                c.on_surface_variant,
            );
            return;
        };

        let (fw, fh) = {
            let sz = *self.shared.video_size.lock().unwrap();
            (sz.0 as f32, sz.1 as f32)
        };
        if fw <= 0.0 || fh <= 0.0 {
            return;
        }

        let scale = (space.width() / fw).min(space.height() / fh);
        let size = vec2(fw * scale, fh * scale);
        let img_rect = Rect::from_center_size(space.center(), size);
        painter.image(
            tex.id(),
            img_rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );

        // Интерактивная зона кадра — без зон «хрома» (полоса сверху 28px,
        // края 8px): в egui-хит-тесте виджет кадра перебивал бы полосу
        // и ресайз-края (низкий слой выигрывает ничью по дистанции).
        let top_h = 28.0;
        let edge = 8.0;
        let body_rect = Rect::from_min_max(
            Pos2::new(img_rect.left() + edge, img_rect.top() + top_h),
            Pos2::new(img_rect.right() - edge, img_rect.bottom() - edge),
        );
        let resp = if body_rect.width() > 4.0 && body_rect.height() > 4.0 {
            ui.allocate_rect(body_rect, Sense::click_and_drag())
        } else {
            ui.allocate_rect(img_rect, Sense::hover())
        };
        let sw = fw as u16;
        let sh = fh as u16;
        let to_dev = |p: Pos2| -> (u32, u32) {
            let x = (((p.x - img_rect.left()) / img_rect.width() * fw).clamp(0.0, fw - 1.0)) as u32;
            let y = (((p.y - img_rect.top()) / img_rect.height() * fh).clamp(0.0, fh - 1.0)) as u32;
            (x, y)
        };

        // Ctrl+ЛКМ — перетащить окно (тач на телефон при этом не идёт)
        let ctrl_held = ui.input(|i| i.modifiers.ctrl);
        if resp.drag_started() && ctrl_held {
            self.begin_window_drag(ctx);
        }
        if resp.dragged() && ctrl_held {
            self.window_drag_frame(ctx);
        }
        self.end_window_drag(ctx);

        let (pressed, down, released, back_clicked, pos) = ui.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.primary_released(),
                i.pointer.secondary_released(),
                i.pointer.latest_pos(),
            )
        });
        let inside = pos.map_or(false, |p| body_rect.contains(p));

        if pressed && inside && !ctrl_held {
            if let Some(p) = pos {
                let (x, y) = to_dev(p);
                self.send_ctrl(ControlMsg::Touch { action: 0, x, y, sw, sh, pressed: true });
                self.touch_captured = Some((x, y));
            }
        }
        if let Some((lx, ly)) = self.touch_captured {
            if down {
                if let Some(p) = pos {
                    let (x, y) = to_dev(p);
                    if (x, y) != (lx, ly) {
                        self.send_ctrl(ControlMsg::Touch { action: 2, x, y, sw, sh, pressed: true });
                        self.touch_captured = Some((x, y));
                    }
                }
            } else {
                let (x, y) = pos.map(to_dev).unwrap_or((lx, ly));
                self.send_ctrl(ControlMsg::Touch { action: 1, x, y, sw, sh, pressed: false });
                self.touch_captured = None;
            }
        }
        let _ = released;
        if back_clicked && inside {
            self.press_key(KEY_BACK);
        }
        // Колесо: один щелчок = 1.0. Windows отдаёт unit Line (±1 за щелчок);
        // unit Point — плавный скролл/тачпад в поинтах (~50 на «строку»).
        let notches = ui.input(|i| {
            i.events
                .iter()
                .filter_map(|ev| match ev {
                    egui::Event::MouseWheel { unit, delta, .. } => Some(match unit {
                        egui::MouseWheelUnit::Line => delta.y,
                        egui::MouseWheelUnit::Page => delta.y * 10.0,
                        _ => delta.y / 50.0,
                    }),
                    _ => None,
                })
                .sum::<f32>()
        });
        if notches != 0.0 {
            // Не полагаемся на egui-hover: он ломается, если какой-то
            // Foreground-виджет перехватил слой или drag-состояние залипло.
            // Для скролла важно только попадание курсора в кадр.
            if let Some(p) = pos.filter(|p| body_rect.contains(*p)) {
                let (x, y) = to_dev(p);
                let v = notches.clamp(-16.0, 16.0);
                self.shared
                    .log(format!("[ctrl] scroll {v:+.2} at ({x},{y}) size {sw}x{sh}"));
                self.send_ctrl(ControlMsg::Scroll { x, y, sw, sh, vscroll: v });
            } else {
                self.shared.log(format!(
                    "[ctrl] scroll пропущен: hovered={} contains={} anydown={} dragged={} img={img_rect:?} pos={pos:?} notches={notches}",
                    resp.hovered(),
                    resp.contains_pointer(),
                    ui.input(|i| i.pointer.any_down()),
                    resp.dragged(),
                ));
            }
        }
    }

    /// Верхняя полоса-хром: перетаскивание окна, клик = меню кнопок,
    /// двойной клик = развернуть. ВАЖНО: это виджет CentralPanel, а не
    /// отдельная Area — egui-хит-тест отдаёт события виджету, добавленному
    /// последним в том же слое, а Foreground-Area перекрывается неявным
    /// move-виджетом других Area и самим кадром.
    fn top_strip(&mut self, ui: &mut Ui, ctx: &egui::Context, c: &Md3) {
        let screen = ctx.screen_rect();
        let top_h = 28.0;
        let top_rect = Rect::from_min_max(
            Pos2::new(screen.left(), screen.top()),
            Pos2::new(screen.right(), screen.top() + top_h),
        );

        let (pos, down, pressed) = ui.input(|i| {
            (
                i.pointer.latest_pos(),
                i.pointer.primary_down(),
                i.pointer.primary_pressed(),
            )
        });
        let in_strip = pos.map_or(false, |p| top_rect.contains(p));

        // «ручка» при наведении
        if in_strip {
            ui.painter().rect_filled(
                Rect::from_center_size(
                    Pos2::new(screen.center().x, screen.top() + 7.0),
                    vec2(64.0, 5.0),
                ),
                Rounding::same(3.0),
                c.on_surface_variant.gamma_multiply(0.55),
            );
        }

        // Сырая обработка: egui-хит-тест ненадёжен в зоне полосы.
        // Нажатие в полосе -> движение = перетащить окно (системный StartDrag),
        // отпускание без движения = показать/скрыть меню кнопок
        // (двойной клик = развернуть).
        if pressed && in_strip {
            self.strip_press = Some((pos.unwrap_or_default(), false));
        }
        if let Some((p0, moved)) = self.strip_press {
            let mut moved = moved;
            if let Some(p) = pos {
                if !moved && (p - p0).length() > 6.0 {
                    moved = true;
                    self.begin_window_drag(ctx);
                }
            }
            if moved {
                self.window_drag_frame(ctx);
            }
            self.end_window_drag(ctx);
            if !down {
                if !moved {
                    let now = std::time::Instant::now();
                    if self
                        .strip_last_click
                        .map(|t| now.duration_since(t) < Duration::from_millis(400))
                        .unwrap_or(false)
                    {
                        self.strip_last_click = None;
                        self.strip_menu = false;
                        self.maximized = !self.maximized;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(self.maximized));
                    } else {
                        self.strip_last_click = Some(now);
                        self.strip_menu = !self.strip_menu;
                        self.shared
                            .log(format!("[strip] меню -> {}", self.strip_menu));
                    }
                }
                self.strip_press = None;
            } else {
                self.strip_press = Some((p0, moved));
            }
        }

        // меню быстрых кнопок — строкой прямо под полосой (абсолютная позиция)
        if self.strip_menu {
            let menu_rect = Rect::from_min_size(
                Pos2::new(screen.left(), screen.top() + top_h + 3.0),
                vec2(screen.width(), 26.0),
            );
            ui.allocate_ui_at_rect(menu_rect, |ui| {
                ui.horizontal(|ui| {
                    ui.style_mut().spacing.item_spacing = vec2(3.0, 0.0);
                    let mut action = false;
                    if strip_button(ui, c, "arrow_back", "Назад") {
                        self.press_key(KEY_BACK);
                        action = true;
                    }
                    if strip_button(ui, c, "home", "Домой") {
                        self.press_key(KEY_HOME);
                        action = true;
                    }
                    if strip_button(ui, c, "apps", "Недавние") {
                        self.press_key(KEY_RECENTS);
                        action = true;
                    }
                    if strip_button(ui, c, "volume_up", "Громче") {
                        self.press_key(KEY_VOLUME_UP);
                        action = true;
                    }
                    if strip_button(ui, c, "volume_down", "Тише") {
                        self.press_key(KEY_VOLUME_DOWN);
                        action = true;
                    }
                    if strip_button(ui, c, "notifications", "Шторка уведомлений") {
                        self.send_ctrl(ControlMsg::ExpandNotifications);
                        action = true;
                    }
                    if strip_button(ui, c, "photo_camera", "Скриншот телефона") {
                        self.take_screenshot();
                        action = true;
                    }
                    if action {
                        self.strip_menu = false;
                    }
                });
            });
        }
    }

    fn status_overlay(&mut self, ui: &mut Ui, c: &Md3) {
        let st = self.shared.status.lock().unwrap().clone();
        let (text, connecting) = match &st {
            Status::Connecting => ("Подключение к телефону…".to_string(), true),
            Status::Failed(e) => (format!("Ошибка: {e}"), false),
            Status::Disconnected | Status::Idle => ("Телефон не подключён".to_string(), false),
            Status::Streaming => (String::new(), false),
        };
        if st == Status::Streaming {
            return;
        }
        ui.add_space(ui.available_height() * 0.42);
        let fill = match &st {
            Status::Failed(_) => c.error_container,
            _ => c.surface_container,
        };
        Frame::none()
            .fill(fill)
            .rounding(Rounding::same(14.0))
            .inner_margin(egui::Margin::symmetric(16.0, 10.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if connecting {
                        ui.add(egui::Spinner::new().size(18.0).color(c.primary));
                        ui.add_space(8.0);
                    }
                    ui.label(RichText::new(&text).size(14.0).color(c.on_surface));
                    ui.add_space(10.0);
                    let label = if connecting { "Отмена" } else { "Повторить" };
                    if ui
                        .add(
                            egui::Button::new(RichText::new(label).size(13.0))
                                .rounding(Rounding::same(16.0)),
                        )
                        .clicked()
                    {
                        if connecting {
                            self.disconnect();
                        } else {
                            self.auto_retry = true;
                            self.refresh_devices();
                            let ctx2 = ui.ctx().clone();
                            self.connect(&ctx2);
                        }
                    }
                });
            });
    }

    fn settings_window(&mut self, ctx: &egui::Context, c: &Md3) {
        let mut open = self.settings_open;
        egui::Window::new(RichText::new("Настройки").size(17.0))
            .open(&mut open)
            .default_width(430.0)
            .frame(
                Frame::none()
                    .fill(c.surface_container)
                    .rounding(Rounding::same(18.0))
                    .inner_margin(egui::Margin::same(16.0)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    md3::card_title(ui, "devices", "Устройство");
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("adb.exe:").size(12.5));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.adb_path_str)
                                .desired_width(ui.available_width() - 110.0),
                        );
                    });
                    ui.horizontal(|ui| {
                        if md3::tonal_button(ui, "Применить").clicked() {
                            let pth = PathBuf::from(self.adb_path_str.trim().trim_matches('"'));
                            let cand = Adb { path: pth };
                            match cand.run(&["version"], 5000) {
                                Ok(_) => {
                                    self.adb = Some(cand);
                                    self.refresh_devices();
                                }
                                Err(e) => self.snack(format!("adb не работает: {}", e)),
                            }
                        }
                        if md3::tonal_button(ui, "Обновить").clicked() {
                            self.refresh_devices();
                        }
                    });
                    for d in &self.devices {
                        ui.horizontal(|ui| {
                            let sel = d.serial == self.selected_serial;
                            if ui.radio(sel, &d.model).clicked() {
                                self.selected_serial = d.serial.clone();
                            }
                            let (fill, fg) = if d.state == "device" {
                                (c.secondary_container, c.success)
                            } else {
                                (c.error_container, c.error)
                            };
                            md3::chip(ui, &d.state, fill, fg);
                        });
                    }

                    ui.add_space(10.0);
                    md3::card_title(ui, "cast", "Поток");
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Битрейт:").size(12.5));
                        ui.add(
                            egui::Slider::new(&mut self.settings.bitrate_mbps, 2..=20)
                                .suffix(" Мбит/с"),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("FPS:").size(12.5));
                        segmented(
                            ui,
                            &[(0u32, "∞"), (30, "30"), (60, "60"), (90, "90"), (120, "120")],
                            &mut self.settings.max_fps,
                            c,
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Размер:").size(12.5));
                        segmented(
                            ui,
                            &[
                                (0u32, "натив"),
                                (1920, "1920"),
                                (1600, "1600"),
                                (1280, "1280"),
                                (1024, "1024"),
                            ],
                            &mut self.settings.max_size,
                            c,
                        );
                    });
                    md3::switch_row(
                        ui,
                        "Показывать касания",
                        "видеть палец на экране",
                        &mut self.settings.show_touches,
                    );
                    md3::switch_row(
                        ui,
                        "Не гасить экран",
                        "stay_awake — пока подключено",
                        &mut self.settings.stay_awake,
                    );

                    ui.add_space(10.0);
                    md3::card_title(ui, "screen_rotation", "Дисплей");
                    md3::switch_row(
                        ui,
                        "Виртуальный дисплей 1920×1080",
                        "scrcpy 3.x: ландшафтный режим",
                        &mut self.settings.virtual_display,
                    );
                    ui.label(
                        RichText::new(if self.settings.virtual_display {
                            "Сервер v3.3.4 (scrcpy-server-v3.jar). Телефон остаётся в портрете."
                        } else {
                            "Сервер v2.7 (scrcpy-server.jar). Зеркало реального экрана."
                        })
                        .size(11.0)
                        .color(c.on_surface_variant),
                    );

                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if md3::filled_button(ui, "Переподключить").clicked() {
                            self.auto_retry = true;
                            self.refresh_devices();
                            let ctx2 = ctx.clone();
                            self.connect(&ctx2);
                        }
                        if md3::tonal_button(ui, "Отключиться").clicked() {
                            self.auto_retry = false;
                            self.disconnect();
                        }
                    });
                });
            });
        self.settings_open = open;
    }

    fn snackbar(&self, ctx: &egui::Context, c: &Md3) {
        let snack = self.shared.snack.lock().unwrap().clone();
        if let Some((text, at)) = snack {
            if at.elapsed() < Duration::from_millis(3500) {
                let screen = ctx.screen_rect();
                let w = (text.chars().count() as f32 * 7.5 + 48.0).min(560.0);
                let rect = Rect::from_center_size(
                    Pos2::new(screen.center().x, screen.bottom() - 40.0),
                    vec2(w, 38.0),
                );
                let painter = egui::Painter::new(
                    ctx.clone(),
                    egui::LayerId::new(egui::Order::Foreground, egui::Id::new("snack")),
                    screen,
                );
                painter.rect_filled(rect, Rounding::same(12.0), c.surface_container_highest);
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    &text,
                    egui::FontId::proportional(13.0),
                    c.on_surface,
                );
            }
        }
    }
    /// Рамка-хром безрисуночная: верхняя полоса = перетаскивание окна,
    /// края/углы = изменение размера. Полупрозрачная «ручка» при наведении.
    fn window_chrome(&mut self, ctx: &egui::Context, c: &Md3) {
        let screen = ctx.screen_rect();
        let edge = 8.0; // толщина краёв ресайза
        let corner = 16.0;
        let _ = c;

        // --- края и углы: изменение размера ---
        let edges: Vec<(egui::Id, Rect, egui::ResizeDirection, egui::CursorIcon)> = vec![
            (
                egui::Id::new("rs_left"),
                Rect::from_min_max(screen.left_top(), Pos2::new(screen.left() + edge, screen.bottom())),
                egui::ResizeDirection::West,
                egui::CursorIcon::ResizeWest,
            ),
            (
                egui::Id::new("rs_right"),
                Rect::from_min_max(Pos2::new(screen.right() - edge, screen.top()), screen.right_bottom()),
                egui::ResizeDirection::East,
                egui::CursorIcon::ResizeEast,
            ),
            (
                egui::Id::new("rs_bottom"),
                Rect::from_min_max(Pos2::new(screen.left(), screen.bottom() - edge), screen.right_bottom()),
                egui::ResizeDirection::South,
                egui::CursorIcon::ResizeSouth,
            ),
            (
                egui::Id::new("rs_nw"),
                Rect::from_min_max(screen.left_top(), Pos2::new(screen.left() + corner, screen.top() + corner)),
                egui::ResizeDirection::NorthWest,
                egui::CursorIcon::ResizeNorthWest,
            ),
            (
                egui::Id::new("rs_ne"),
                Rect::from_min_max(Pos2::new(screen.right() - corner, screen.top()), Pos2::new(screen.right(), screen.top() + corner)),
                egui::ResizeDirection::NorthEast,
                egui::CursorIcon::ResizeNorthEast,
            ),
            (
                egui::Id::new("rs_sw"),
                Rect::from_min_max(Pos2::new(screen.left(), screen.bottom() - corner), Pos2::new(screen.left() + corner, screen.bottom())),
                egui::ResizeDirection::SouthWest,
                egui::CursorIcon::ResizeSouthWest,
            ),
            (
                egui::Id::new("rs_se"),
                Rect::from_min_max(Pos2::new(screen.right() - corner, screen.bottom() - corner), screen.right_bottom()),
                egui::ResizeDirection::SouthEast,
                egui::CursorIcon::ResizeSouthEast,
            ),
        ];
        for (id, rect, dir, cursor) in edges {
            egui::Area::new(id)
                .fixed_pos(rect.min)
                .movable(false)
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    let resp = ui.allocate_rect(rect, Sense::click_and_drag());
                    if resp.hovered() {
                        ui.output_mut(|o| o.cursor_icon = cursor);
                    }
                    if resp.drag_started() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
                    }
                });
        }
    }



}

/// Иконка-кнопка в меню верхней полосы.
fn strip_button(ui: &mut Ui, c: &Md3, icon: &str, tip: &str) -> bool {
    let t = RichText::new(icons::icon(icon))
        .font(icons::material_font(16.0))
        .color(c.on_surface);
    ui.add(
        egui::Button::new(t)
            .min_size(vec2(30.0, 22.0))
            .rounding(Rounding::same(8.0))
            .fill(c.surface_container_highest),
    )
    .on_hover_text(tip)
    .clicked()
}

/// иконка трея: фиолетовый скруглённый квадрат + белая «трубка»
fn draw_tray_icon(sz: u8) -> (u8, Vec<u8>) {
    let s = sz as i32;
    let mut rgba = vec![0u8; (s * s * 4) as usize];
    let r = 8.0f32;
    let put = |x: i32, y: i32, col: [u8; 4], rgba: &mut Vec<u8>| {
        let idx = ((y * s + x) * 4) as usize;
        if idx + 3 < rgba.len() {
            rgba[idx] = col[0];
            rgba[idx + 1] = col[1];
            rgba[idx + 2] = col[2];
            rgba[idx + 3] = col[3];
        }
    };
    for y in 0..s {
        for x in 0..s {
            let dx = (x as f32 + 0.5 - s as f32 / 2.0).abs();
            let dy = (y as f32 + 0.5 - s as f32 / 2.0).abs();
            let half = s as f32 / 2.0 - 1.0;
            let in_round = dx <= half - r
                || dy <= half - r
                || ((dx - (half - r)).powi(2) + (dy - (half - r)).powi(2)).sqrt() <= r;
            if in_round {
                let in_phone = x >= s / 2 - 5
                    && x < s / 2 + 5
                    && y >= 6
                    && y < s - 7
                    && !(y >= s / 2 - 3 && y < s / 2 - 1 && x >= s / 2 - 2 && x < s / 2 + 2);
                if in_phone {
                    put(x, y, [255, 255, 255, 255], &mut rgba);
                } else {
                    put(x, y, [103, 80, 164, 255], &mut rgba);
                }
            } else {
                put(x, y, [0, 0, 0, 0], &mut rgba);
            }
        }
    }
    (sz, rgba)
}
/// Сегментированный выбор (MD3).
fn segmented<T: PartialEq + Copy>(ui: &mut Ui, options: &[(T, &str)], current: &mut T, c: &Md3) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (val, label) in options {
            let selected = *val == *current;
            let resp = ui.add(
                egui::Button::new(
                    RichText::new(*label).size(12.5).color(if selected {
                        c.on_secondary_container
                    } else {
                        c.on_surface_variant
                    }),
                )
                .fill(if selected {
                    c.secondary_container
                } else {
                    c.surface_container_high
                })
                .rounding(Rounding::same(9.0))
                .min_size(vec2(42.0, 26.0)),
            );
            if resp.clicked() {
                *current = *val;
            }
        }
    });
}
impl eframe::App for MirrorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let c = self.colors();
        md3::set_colors(ctx, &c);
        md3::apply(ctx, &c);

        self.handle_tray_events(ctx);
        self.poll_shared();
        self.update_texture(ctx);

        // авто-переподключение по таймеру
        if self.auto_retry
            && self.conn.is_none()
            && self
                .next_retry
                .map(|t| std::time::Instant::now() >= t)
                .unwrap_or(false)
        {
            self.next_retry = None;
            self.refresh_devices();
            self.connect(ctx);
        }

        egui::CentralPanel::default()
            .frame(Frame::none().fill(Color32::BLACK))
            .show(ctx, |ui| {
                let rect = ui.available_rect_before_wrap();
                self.video_area(ui, rect, ctx, &c);
                // полоса — после кадра: поздний виджет в том же слое
                // получает клики/драг в зоне полосы
                self.top_strip(ui, ctx, &c);
            });

        // клавиатура ПК → телефон в режиме SDK:
        // печатаемые символы — INJECT_TEXT (любая раскладка/язык),
        // комбо с Ctrl/Alt/Meta и спецклавиши — INJECT_KEYCODE
        if self.keyboard_enabled && !self.settings_open {
            let events = ctx.input(|i| i.events.clone());
            let mut cmd_held = false; // Ctrl/Alt/Meta — по последнему Key-событию
            for ev in &events {
                match ev {
                    egui::Event::Key { key, pressed, repeat, modifiers, .. } => {
                        cmd_held = keyboard::has_cmd(*modifiers);
                        let meta = keyboard::meta_state(*modifiers);
                        let Some(kc) = keyboard::key_to_android(*key) else {
                            continue;
                        };
                        if !*pressed {
                            self.send_ctrl(ControlMsg::KeyCode {
                                action: 1,
                                keycode: kc,
                                meta_state: meta,
                                repeat: 0,
                            });
                            continue;
                        }
                        if *repeat {
                            // авто-повтор важен для стрелок/backspace/комбо;
                            // печатаемые повторяются через Event::Text
                            if keyboard::is_special_key(*key) || cmd_held {
                                self.send_ctrl(ControlMsg::KeyCode {
                                    action: 0,
                                    keycode: kc,
                                    meta_state: meta,
                                    repeat: 1,
                                });
                            }
                            continue;
                        }
                        if cmd_held || keyboard::is_special_key(*key) {
                            self.shared.log(format!(
                                "[kb] keycode {:?} -> {} meta={} cmd={cmd_held}",
                                key, kc, meta
                            ));
                            self.send_ctrl(ControlMsg::KeyCode {
                                action: 0,
                                keycode: kc,
                                meta_state: meta,
                                repeat: 0,
                            });
                        }
                        // иначе: печатаемый символ — уйдёт через Event::Text
                    }
                    egui::Event::Text(text) => {
                        if !cmd_held {
                            self.shared.log(format!("[kb] text {:?} cmd={cmd_held}", text));
                            // режем только очень длинные вставки (обычно 1 символ),
                            // по границам символов
                            for chunk in text.chars().collect::<Vec<_>>().chunks(100) {
                                self.send_ctrl(ControlMsg::InjectText(
                                    chunk.iter().collect(),
                                ));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // безрамочный «хром»: перетаскивание сверху, ресайз за края
        self.window_chrome(ctx, &c);

        // оверлей статуса (когда нет стрима)
        let st = self.shared.status.lock().unwrap().clone();
        if st != Status::Streaming {
            egui::Area::new(egui::Id::new("status_overlay"))
                .anchor(egui::Align2::CENTER_TOP, vec2(0.0, 14.0))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    self.status_overlay(ui, &c);
                });
        }

        if self.settings_open {
            self.settings_window(ctx, &c);
        }

        self.snackbar(ctx, &c);
        ctx.request_repaint_after(Duration::from_millis(400));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.disconnect();
    }
}

fn main() -> eframe::Result<()> {
    // иконка окна/панели задач — та же пиксельная «трубка», что и в трее
    let (icon_sz, icon_rgba) = draw_tray_icon(64);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([420.0, 930.0])
            .with_min_inner_size([200.0, 400.0])
            .with_decorations(false) // без рамок и заголовка
            .with_resizable(true) // ресайз за края (winit undecorated hit-test)
            .with_title("Android Mirror")
            .with_icon(egui::IconData {
                width: icon_sz as u32,
                height: icon_sz as u32,
                rgba: icon_rgba,
            }),
        ..Default::default()
    };
    eframe::run_native(
        "Android Mirror",
        options,
        Box::new(|cc| Ok(Box::new(MirrorApp::new(cc)))),
    )
}
