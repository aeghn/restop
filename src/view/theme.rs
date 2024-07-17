use std::sync::Arc;

use ratatui::style::{Color, Modifier, Style};

pub type SharedTheme = Arc<Theme>;

#[derive(Debug, Clone)]
pub struct Theme {
    focused_fg: Color,
    inactive_fg: Color,
    label_fg: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            focused_fg: Color::Reset,
            label_fg: Color::DarkGray,
            inactive_fg: Color::DarkGray,
        }
    }
}

impl Theme {
    pub fn fg(&self, focused: bool) -> Color {
        if focused {
            self.focused_fg
        } else {
            self.inactive_fg
        }
    }

    pub fn title(&self, focused: bool) -> Style {
        if focused {
            Style::default()
                .fg(self.focused_fg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(self.inactive_fg)
        }
    }

    pub fn value(&self, focused: bool) -> Style {
        if focused {
            Style::default().fg(self.focused_fg)
        } else {
            Style::default().fg(self.inactive_fg)
        }
    }

    pub fn key(&self, focused: bool) -> Style {
        if focused {
            Style::default()
                .fg(self.label_fg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(self.label_fg)
        }
    }
}
