use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub fn is_ctrl_c(key: &KeyEvent) -> bool {
    KeyCode::Char('c') == key.code && (key.modifiers == KeyModifiers::CONTROL)
}

pub fn is_esc(key: &KeyEvent) -> bool {
    KeyCode::Esc == key.code && key.modifiers.is_empty()
}

pub fn is_q(key: &KeyEvent) -> bool {
    KeyCode::Char('q') == key.code && key.modifiers.is_empty()
}

pub fn is_char_and_mod(key: &KeyEvent, c: char, modifier: KeyModifiers) -> bool {
    KeyCode::Char(c) == key.code && (key.modifiers == modifier)
}

pub fn is_only_char(key: &KeyEvent, c: char) -> bool {
    KeyCode::Char(c) == key.code && key.modifiers.is_empty()
}

pub fn is_alt_char(key: &KeyEvent, c: char) -> bool {
    KeyCode::Char(c) == key.code && key.modifiers == KeyModifiers::ALT
}
