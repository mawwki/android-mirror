// Ввод с клавиатуры ПК в режиме SDK (как --keyboard=sdk в scrcpy):
// печатаемые символы -> TYPE_INJECT_TEXT (Unicode: раскладка и язык не важны —
// кириллица работает при любой раскладке на телефоне),
// клавиши и комбинации с Ctrl/Alt/Meta -> TYPE_INJECT_KEYCODE
// с кодами android.view.KeyEvent.

/// egui::Key -> Android KEYCODE (android.view.KeyEvent)
pub fn key_to_android(key: egui::Key) -> Option<u32> {
    use egui::Key::*;
    Some(match key {
        A => 29,
        B => 30,
        C => 31,
        D => 32,
        E => 33,
        F => 34,
        G => 35,
        H => 36,
        I => 37,
        J => 38,
        K => 39,
        L => 40,
        M => 41,
        N => 42,
        O => 43,
        P => 44,
        Q => 45,
        R => 46,
        S => 47,
        T => 48,
        U => 49,
        V => 50,
        W => 51,
        X => 52,
        Y => 53,
        Z => 54,
        Num0 => 7,
        Num1 => 8,
        Num2 => 9,
        Num3 => 10,
        Num4 => 11,
        Num5 => 12,
        Num6 => 13,
        Num7 => 14,
        Num8 => 15,
        Num9 => 16,
        Enter => 66,
        Backspace => 67,
        Tab => 61,
        Space => 62,
        Escape => 111,
        Minus => 69,
        Equals => 70,
        Comma => 55,
        Period => 56,
        OpenBracket => 71,
        CloseBracket => 72,
        Backslash => 73,
        Semicolon => 74,
        Quote => 75,
        Backtick => 68,
        Slash => 76,
        ArrowRight => 22,
        ArrowLeft => 21,
        ArrowDown => 20,
        ArrowUp => 19,
        Insert => 124,
        Home => 122,
        PageUp => 92,
        Delete => 112,
        End => 123,
        PageDown => 93,
        F1 => 131,
        F2 => 132,
        F3 => 133,
        F4 => 134,
        F5 => 135,
        F6 => 136,
        F7 => 137,
        F8 => 138,
        F9 => 139,
        F10 => 140,
        F11 => 141,
        F12 => 142,
        _ => return None,
    })
}

/// Биты META_*_ON из android.view.KeyEvent
pub const META_SHIFT_ON: u32 = 0x1;
pub const META_ALT_ON: u32 = 0x2;
pub const META_CTRL_ON: u32 = 0x1000;
pub const META_META_ON: u32 = 0x400_0000;

/// egui::Modifiers -> Android metaState
pub fn meta_state(m: egui::Modifiers) -> u32 {
    let mut s = 0u32;
    if m.shift {
        s |= META_SHIFT_ON;
    }
    if m.alt {
        s |= META_ALT_ON;
    }
    if m.ctrl {
        s |= META_CTRL_ON;
    }
    if m.mac_cmd {
        s |= META_META_ON;
    }
    s
}

/// Есть ли командный модификатор: такие нажатия идут через KEYCODE,
/// а не через текст (как фильтр is_cmd в egui-winit).
pub fn has_cmd(m: egui::Modifiers) -> bool {
    m.ctrl || m.alt || m.mac_cmd
}

/// «Специальная» клавиша: не печатает символ, вводится KEYCODE даже без
/// модификаторов. Печатаемые (буквы/цифры/пунктуация/пробел) идут через
/// Event::Text -> INJECT_TEXT, чтобы не задваиваться с keycode.
pub fn is_special_key(key: egui::Key) -> bool {
    use egui::Key::*;
    matches!(
        key,
        Enter
            | Backspace
            | Delete
            | Insert
            | Tab
            | Escape
            | ArrowUp
            | ArrowDown
            | ArrowLeft
            | ArrowRight
            | Home
            | End
            | PageUp
            | PageDown
            | F1
            | F2
            | F3
            | F4
            | F5
            | F6
            | F7
            | F8
            | F9
            | F10
            | F11
            | F12
    )
}
