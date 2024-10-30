use std::ops::Add;

use itertools::Itertools;
use ratatui::{
    style::{Color, Modifier, Stylize},
    symbols::line::*,
    text::{Line, Span},
    widgets::Scrollbar,
    Frame,
};

use crate::ring::{self, Ring};
use chin_tools::utils::stringutils::split_by_len;
use ratatui::style::Style;

pub mod grouped_lines;
pub mod input;
pub mod stateful_lines;

pub fn ls_kv(
    key: Option<&str>,
    value: &str,
    width: u16,
    key_style: Style,
    value_style: Style,
) -> Vec<Line<'static>> {
    if width <= 0 {
        return vec![];
    }

    if let Some(key) = key {
        if key.len() + value.len() + 1 < width as usize {
            vec![vec![
                s_label(key, key_style),
                s_label(" ", value_style),
                s_value(value, value_style),
            ]
            .into()]
        } else {
            let mut lines = vec![];

            split_by_len(key, width as usize)
                .iter()
                .for_each(|e| lines.push(Line::from(s_label(e, key_style))));

            split_by_len(value, width as usize)
                .iter()
                .for_each(|e| lines.push(Line::from(s_value(e, value_style))));

            lines
        }
    } else {
        if value.len() < width as usize {
            vec![vec![s_value(value, value_style)].into()]
        } else {
            let mut lines = vec![];

            split_by_len(value, width as usize)
                .iter()
                .for_each(|e| lines.push(Line::from(s_value(e, value_style))));

            lines
        }
    }
}

pub fn s_percent_graph(value: f64, total: f64, width: u16) -> Vec<Span<'static>> {
    let percent = (value * 100. / total) as u16;
    let graph_width = width.saturating_sub((2) as u16);

    let mut colors = [
        Color::DarkGray,
        Color::Blue,
        Color::Cyan,
        Color::Green,
        Color::Yellow,
        Color::Magenta,
        Color::Red,
    ];
    colors.reverse();

    let value_width = graph_width as usize * percent as usize / 100;
    let color = percent / (100 / colors.len() as u16);

    let color = if color <= 0 {
        colors[0]
    } else if color as usize >= colors.len() {
        *colors.last().unwrap()
    } else {
        colors[color as usize]
    };

    let legent_width = if percent < 10 {
        1
    } else if percent < 100 {
        2
    } else {
        3
    };

    let value_width = std::cmp::min(value_width as u16, graph_width.saturating_sub(legent_width));
    let padding_width = graph_width
        .saturating_sub(legent_width)
        .saturating_sub(value_width);

    let mut graph = String::new();

    for _ in 0..value_width {
        graph.push('|');
    }

    for _ in 0..padding_width {
        graph.push(' ');
    }

    vec![
        Span::raw("["),
        Span::styled(graph, Style::new().fg(color)),
        Span::raw(format!("{}", percent)),
        Span::raw("]"),
    ]
}

pub fn s_hotgraph<'r>(
    width: u16,
    ring: &Ring<f64>,
    max_value: f64,
    min_value: f64,
    line_height: u16,
    color: Color,
) -> Vec<Span<'static>> {
    let width = width;

    const MAX_HEIGHT: usize = 4;

    let bars = [
        [' ', '⢀', '⢠', '⢰', '⢸'],
        ['⡀', '⣀', '⣠', '⣰', '⣸'],
        ['⡄', '⣄', '⣤', '⣴', '⣼'],
        ['⡆', '⣆', '⣦', '⣶', '⣾'],
        ['⡇', '⣇', '⣧', '⣷', '⣿'],
    ];

    let bar_sep = (max_value - min_value) / (MAX_HEIGHT as i32 * line_height as i32) as f64;

    let mut lines: Vec<String> = vec![];
    for _ in 0..line_height {
        lines.push(String::with_capacity(width.into()));
    }

    let values: Vec<&f64> = ring.new_to_old_iter().take(width as usize * 2).collect();

    values.chunks(2).for_each(|e| {
        let left = e
            .get(0)
            .map(|e| ((**e - min_value) / bar_sep).round() as usize)
            .unwrap_or(0);
        let right = e
            .get(1)
            .map(|e| ((**e - min_value) / bar_sep).round() as usize)
            .unwrap_or(0);

        for i in 1..=line_height {
            let max = i as usize * MAX_HEIGHT as usize;
            let r = if max <= left {
                MAX_HEIGHT
            } else {
                (left + MAX_HEIGHT).saturating_sub(max)
            };
            let l = if max <= right {
                MAX_HEIGHT
            } else {
                (right + MAX_HEIGHT).saturating_sub(max)
            };

            lines.get_mut(i as usize - 1).and_then(|t| {
                let mut sym = bars.get(l).and_then(|v| v.get(r)).unwrap_or(&'?');
                if i == 1 && line_height > 1 {
                    sym = bars
                        .get(l.add(1).clamp(1, MAX_HEIGHT))
                        .and_then(|v| v.get(r.add(1).clamp(1, MAX_HEIGHT)))
                        .unwrap_or(&'?');
                }
                t.insert(0, *sym);
                Some(())
            });
        }
    });

    let true_len = (values.len() + 1) / 2;
    if (width as usize) >= true_len {
        let limit = if line_height > 1 { 1 } else { 0 };
        for line in &mut lines.iter_mut().take(limit) {
            for _ in 0..((width as usize).saturating_sub(true_len)) {
                line.insert(0, '⣀');
            }
        }

        for line in &mut lines.iter_mut().skip(limit) {
            for _ in 0..((width as usize).saturating_sub(true_len)) {
                line.insert(0, ' ');
            }
        }
    };

    lines
        .into_iter()
        .rev()
        .map(|e| Span::from(e).fg(color))
        .collect()
}

pub fn ls_hotgraph<'r>(
    width: u16,
    ring: &Ring<f64>,
    max_value: f64,
    min_value: f64,
    line_height: u16,
    color: Color,
) -> Vec<Line<'static>> {
    s_hotgraph(width, ring, max_value, min_value, line_height, color)
        .into_iter()
        .map(|e| Line::from(e))
        .collect()
}

pub fn s_label(label: &str, style: Style) -> Span<'static> {
    Span::styled(String::from(label), style)
}

pub fn s_value(label: &str, style: Style) -> Span<'static> {
    Span::styled(String::from(label), style)
}

pub fn ls_italic(label: &str, width: u16) -> Vec<Line<'static>> {
    ls_kv(
        None,
        &label,
        width,
        Style::new(),
        Style::new().add_modifier(Modifier::ITALIC),
    )
}

pub fn ls_common(label: &str, width: u16) -> Vec<Line<'static>> {
    ls_kv(None, &label, width, Style::new(), Style::new())
}

pub fn ls_style(label: &str, width: u16, style: Style) -> Vec<Line<'static>> {
    ls_kv(None, &label, width, Style::new(), style)
}

pub trait PaddingH {
    fn padding(self) -> Self;
}

impl PaddingH for Vec<Span<'static>> {
    fn padding(self) -> Self {
        let mut this = self;
        this.insert(0, Span::raw(" "));
        this.push(Span::raw(" "));
        this
    }
}

pub fn render_border(
    focused: bool,
    area: ratatui::prelude::Rect,
    buf: &mut ratatui::prelude::Buffer,
) {
    let tl;
    let tr;
    let hor;
    let ver;
    let bl;
    let br;
    match focused {
        false => {
            tl = TOP_LEFT;
            tr = TOP_RIGHT;
            hor = HORIZONTAL;
            ver = VERTICAL;
            bl = BOTTOM_LEFT;
            br = BOTTOM_RIGHT;
        }
        true => {
            tl = DOUBLE_TOP_LEFT;
            tr = DOUBLE_TOP_RIGHT;
            hor = DOUBLE_HORIZONTAL;
            ver = DOUBLE_VERTICAL;
            bl = DOUBLE_BOTTOM_LEFT;
            br = DOUBLE_BOTTOM_RIGHT;
        }
    }

    let top = area.top();
    let right = area.right().saturating_sub(1);
    let bot = area.bottom().saturating_sub(1);
    let left = area.left();

    buf.set_string(left, top, tl, Style::new());
    buf.set_string(right, top, tr, Style::new());

    buf.set_string(left, bot, bl, Style::new());
    buf.set_string(right, bot, br, Style::new());

    for i in left.saturating_add(1)..right {
        buf.set_string(i, top, hor, Style::new());
    }
    for i in left.saturating_add(1)..right {
        buf.set_string(i, bot, hor, Style::new());
    }
    for i in top.saturating_add(1)..bot {
        buf.set_string(left, i, ver, Style::new());
    }
    for i in top.saturating_add(1)..bot {
        buf.set_string(right, i, ver, Style::new());
    }
}
