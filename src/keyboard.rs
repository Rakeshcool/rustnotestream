use std::thread;
use std::time::Duration;
use windows::Win32::UI::Input::KeyboardAndMouse::*;

const KEY_DOWN: KEYBD_EVENT_FLAGS = KEYBD_EVENT_FLAGS(0);

/// Simulate typing a string of text, with optional Enter key press at the end.
pub fn simulate_typing(text: &str, press_enter: bool) {
    if text.is_empty() {
        return;
    }

    for ch in text.chars() {
        if let Some((vk, needs_shift)) = char_to_vk(ch) {
            if needs_shift {
                unsafe { keybd_event(VK_SHIFT.0 as u8, 0, KEY_DOWN, 0); }
            }
            press_key(vk);
            if needs_shift {
                unsafe { keybd_event(VK_SHIFT.0 as u8, 0, KEYEVENTF_KEYUP, 0); }
            }
        }
        thread::sleep(Duration::from_millis(10));
    }

    if press_enter {
        thread::sleep(Duration::from_millis(50));
        press_key(VK_RETURN.0 as u8);
    }
}

/// Simulate pressing a special key (backspace, delete, tab, esc, enter, etc.).
pub fn simulate_special_key(key: &str) {
    if let Some(vk) = special_key_to_vk(key) {
        press_key(vk);
    }
}

/// Simulate a key combination (hotkey), e.g. Ctrl+C, Alt+Tab.
pub fn simulate_key_combo(keys: &[String]) {
    if keys.is_empty() {
        return;
    }

    let vks: Vec<u8> = keys.iter().filter_map(|k| modifier_or_key_to_vk(k)).collect();
    if vks.is_empty() {
        return;
    }

    for &vk in &vks {
        unsafe { keybd_event(vk, 0, KEY_DOWN, 0); }
    }

    thread::sleep(Duration::from_millis(30));

    for &vk in vks.iter().rev() {
        unsafe { keybd_event(vk, 0, KEYEVENTF_KEYUP, 0); }
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn press_key(vk: u8) {
    unsafe {
        keybd_event(vk, 0, KEY_DOWN, 0);
        keybd_event(vk, 0, KEYEVENTF_KEYUP, 0);
    }
}

fn char_to_vk(ch: char) -> Option<(u8, bool)> {
    if ('a'..='z').contains(&ch) {
        let code = (VK_A.0 as u8) + (ch as u8 - b'a');
        return Some((code, false));
    }
    if ('A'..='Z').contains(&ch) {
        let code = (VK_A.0 as u8) + (ch as u8 - b'A');
        return Some((code, true));
    }
    if ('0'..='9').contains(&ch) {
        let code = (VK_0.0 as u8) + (ch as u8 - b'0');
        return Some((code, false));
    }

    match ch {
        ' ' => Some((VK_SPACE.0 as u8, false)),
        ';' => Some((0xBA, false)),
        '=' => Some((0xBB, false)),
        ',' => Some((0xBC, false)),
        '-' => Some((0xBD, false)),
        '.' => Some((0xBE, false)),
        '/' => Some((0xBF, false)),
        '`' => Some((0xC0, false)),
        '[' => Some((0xDB, false)),
        '\\' => Some((0xDC, false)),
        ']' => Some((0xDD, false)),
        '\'' => Some((0xDE, false)),

        '!' => Some((VK_1.0 as u8, true)),
        '@' => Some((VK_2.0 as u8, true)),
        '#' => Some((VK_3.0 as u8, true)),
        '$' => Some((VK_4.0 as u8, true)),
        '%' => Some((VK_5.0 as u8, true)),
        '^' => Some((VK_6.0 as u8, true)),
        '&' => Some((VK_7.0 as u8, true)),
        '*' => Some((VK_8.0 as u8, true)),
        '(' => Some((VK_9.0 as u8, true)),
        ')' => Some((VK_0.0 as u8, true)),
        ':' => Some((0xBA, true)),
        '+' => Some((0xBB, true)),
        '<' => Some((0xBC, true)),
        '_' => Some((0xBD, true)),
        '>' => Some((0xBE, true)),
        '?' => Some((0xBF, true)),
        '~' => Some((0xC0, true)),
        '{' => Some((0xDB, true)),
        '|' => Some((0xDC, true)),
        '}' => Some((0xDD, true)),
        '"' => Some((0xDE, true)),
        _ => None,
    }
}

fn special_key_to_vk(key: &str) -> Option<u8> {
    match key.to_lowercase().as_str() {
        "backspace" => Some(VK_BACK.0 as u8),
        "delete" => Some(VK_DELETE.0 as u8),
        "tab" => Some(VK_TAB.0 as u8),
        "escape" => Some(VK_ESCAPE.0 as u8),
        "enter" => Some(VK_RETURN.0 as u8),
        "space" => Some(VK_SPACE.0 as u8),
        "up" => Some(VK_UP.0 as u8),
        "down" => Some(VK_DOWN.0 as u8),
        "left" => Some(VK_LEFT.0 as u8),
        "right" => Some(VK_RIGHT.0 as u8),
        "home" => Some(VK_HOME.0 as u8),
        "end" => Some(VK_END.0 as u8),
        "pageup" => Some(VK_PRIOR.0 as u8),
        "pagedown" => Some(VK_NEXT.0 as u8),
        "capslock" => Some(VK_CAPITAL.0 as u8),
        _ => None,
    }
}

fn modifier_or_key_to_vk(key: &str) -> Option<u8> {
    match key.to_lowercase().as_str() {
        "ctrl" => Some(VK_CONTROL.0 as u8),
        "control" => Some(VK_CONTROL.0 as u8),
        "alt" => Some(VK_MENU.0 as u8),
        "shift" => Some(VK_SHIFT.0 as u8),
        "win" => Some(VK_LWIN.0 as u8),
        "windows" => Some(VK_LWIN.0 as u8),
        "meta" => Some(VK_LWIN.0 as u8),
        k => special_key_to_vk(k),
    }
}
