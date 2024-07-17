use ratatui::{
    style::{Color, Modifier, Stylize},
    text::{Line, Span},
};

use crate::ring::Ring;
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
) -> Vec<Span<'static>> {
    let width = width;

    let bars = [
        "⡀", "⣀", "⡄", "⣄", "⣠", "⣤", "⡆", "⣆", "⣰", "⣦", "⣴", "⣶", "⡇", "⣇", "⣸", "⣧", "⣼", "⣷",
        "⣾", "⣿",
    ];
    let colors = [
        Color::DarkGray,
        Color::Blue,
        Color::Cyan,
        Color::Green,
        Color::Yellow,
        Color::Magenta,
        Color::Red,
        Color::Blue,
        Color::Cyan,
        Color::Green,
        Color::Yellow,
        Color::Magenta,
        Color::Red,
        Color::Blue,
        Color::Cyan,
        Color::Green,
        Color::Yellow,
        Color::Magenta,
        Color::Red,
    ];

    let bar_sep = (max_value - min_value) / bars.len() as f64;
    let color_sep = (max_value - min_value) / colors.len() as f64;

    let get_bar = |e: f64| {
        let height = ((e - min_value) / bar_sep).round() as i64;
        let color = ((e - min_value) / color_sep).round() as i64;

        let bar = if height <= 1 {
            &bars[0]
        } else if height - 1 >= bars.len() as i64 {
            bars.last().unwrap()
        } else {
            &bars[(height - 1) as usize]
        };

        let color = if color <= 1 {
            &colors[0]
        } else if height - 1 >= colors.len() as i64 {
            colors.last().unwrap()
        } else {
            &colors[(height - 1) as usize]
        };

        Span::raw(*bar).fg(*color)
    };

    let mut spans = Vec::with_capacity((width) as usize);

    let ring_iter = ring.all();

    if (width as usize) >= ring_iter.len() {
        let len = ring_iter.len();
        spans.extend(ring_iter.map(|e| get_bar(*e)));
        for _ in 0..((width as usize).saturating_sub(len)) {
            spans.push(get_bar(min_value))
        }
    } else {
        spans.extend(ring_iter.take(width as usize).map(|e| get_bar(*e)))
    };

    spans
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
