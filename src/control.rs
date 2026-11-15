// Control-протокол scrcpy v2.7: сообщения клиент -> устройство.
// Раскладка байтов сверена с ControlMessageReader.java (v2.7).
use std::io::Write;
use std::net::TcpStream;

#[derive(Clone, Debug)]
pub enum ControlMsg {
    /// TYPE_INJECT_KEYCODE (0)
    KeyCode { action: u8, keycode: u32, meta_state: u32, repeat: u32 },
    /// TYPE_INJECT_TOUCH_EVENT (2). Координаты — в пикселях ВИДЕОКАДРА,
    /// screen size = размер кадра (требование Device.getPhysicalPoint v2.7).
    Touch { action: u8, x: u32, y: u32, sw: u16, sh: u16, pressed: bool },
    /// TYPE_INJECT_SCROLL_EVENT (3), vscroll в «шагах» (1.0 = один щелчок колеса).
    Scroll { x: u32, y: u32, sw: u16, sh: u16, vscroll: f32 },
    /// TYPE_BACK_OR_SCREEN_ON (4) — Back или разбудить экран.
    BackScreenOn,
    /// TYPE_EXPAND_NOTIFICATION_PANEL (5)
    ExpandNotifications,
    /// TYPE_EXPAND_SETTINGS_PANEL (6)
    ExpandSettings,
    /// TYPE_COLLAPSE_PANELS (7)
    CollapsePanels,
    /// TYPE_INJECT_TEXT (1)
    InjectText(String),
    /// TYPE_SET_SCREEN_POWER_MODE (10): 0 = off, 1 = normal
    ScreenPower(u8),
    /// TYPE_ROTATE_DEVICE (11)
    RotateDevice,
}

impl ControlMsg {
    pub fn encode_into(&self, out: &mut Vec<u8>) {
        match self {
            ControlMsg::KeyCode { action, keycode, meta_state, repeat } => {
                out.push(0);
                out.push(*action);
                out.extend(keycode.to_be_bytes()); // keycode
                out.extend(repeat.to_be_bytes()); // repeat
                out.extend(meta_state.to_be_bytes()); // metaState
            }
            ControlMsg::Touch { action, x, y, sw, sh, pressed } => {
                out.push(2);
                out.push(*action);
                out.extend((-1i64).to_be_bytes()); // pointerId: мышь = -1
                out.extend(x.to_be_bytes());
                out.extend(y.to_be_bytes());
                out.extend((*sw).to_be_bytes());
                out.extend((*sh).to_be_bytes());
                out.extend(if *pressed { 0xFFFFu16 } else { 0 }.to_be_bytes()); // pressure
                out.extend(if *pressed { 1u32 } else { 0 }.to_be_bytes()); // actionButton
                out.extend(if *pressed { 1u32 } else { 0 }.to_be_bytes()); // buttons
            }
            ControlMsg::Scroll { x, y, sw, sh, vscroll } => {
                out.push(3);
                out.extend(x.to_be_bytes());
                out.extend(y.to_be_bytes());
                out.extend((*sw).to_be_bytes());
                out.extend((*sh).to_be_bytes());
                out.extend(0i16.to_be_bytes()); // hScroll
                // i16 fixed-point: масштаб ×32768 (sc_float_to_i16fp, 15 дробных
                // бит); 1.0 = один щелчок колеса. Сервер декодирует v/32768.
                let v = (*vscroll * 32768.0).clamp(-32768.0, 32767.0) as i16;
                out.extend(v.to_be_bytes()); // vScroll (i16 fixed point)
                out.extend(0u32.to_be_bytes()); // buttons
            }
            ControlMsg::BackScreenOn => {
                out.push(4);
                out.push(0); // ACTION_DOWN
            }
            ControlMsg::ExpandNotifications => out.push(5),
            ControlMsg::ExpandSettings => out.push(6),
            ControlMsg::CollapsePanels => out.push(7),
            ControlMsg::InjectText(s) => {
                out.push(1);
                let bytes = s.as_bytes();
                out.extend((bytes.len() as u32).to_be_bytes());
                out.extend_from_slice(bytes);
            }
            ControlMsg::ScreenPower(mode) => {
                out.push(10);
                out.push(*mode);
            }
            ControlMsg::RotateDevice => out.push(11),
        }
    }
}

/// Поток-писатель control-сокета.
pub fn control_writer(mut stream: TcpStream, rx: std::sync::mpsc::Receiver<ControlMsg>) {
    for msg in rx {
        let mut buf = Vec::with_capacity(64);
        msg.encode_into(&mut buf);
        if stream.write_all(&buf).and_then(|_| stream.flush()).is_err() {
            break;
        }
    }
}
