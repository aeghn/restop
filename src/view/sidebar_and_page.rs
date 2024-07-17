use crossterm::event::KeyCode;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    Frame,
};

use crate::{component::stateful_lines::StatefulGroupedLines, resource::ResourceType};

use super::{Navigator, NavigatorArgs, OverviewArg, PageArg};

#[derive(Debug, Default)]
pub struct SidebarAndPage {
    pub sidebar: Rect,
    pub sidebar_state: StatefulGroupedLines<'static>,
    pub page: Rect,
    pub page_focused: bool,
}

impl SidebarAndPage {
    fn overview(&mut self, frame: &mut Frame, resources: &mut Vec<ResourceType>) {
        let rect = self.sidebar.clone();

        let mut args = OverviewArg {
            width: rect.width.clone(),
            focused: !self.page_focused,
        };
        let mut overviews = vec![];
        for ele in resources.iter() {
            if let Ok(ov) = ele.overview_content(&mut args) {
                overviews.push(ov)
            }
        }
        self.sidebar_state.update_blocks(overviews);
        self.sidebar_state.render(frame, rect.clone());
    }
}

impl Navigator for SidebarAndPage {
    fn update_layout(&mut self, rect: Rect) {
        let lr = Layout::default()
            .direction(ratatui::layout::Direction::Horizontal)
            .constraints([
                Constraint::Fill(1),
                Constraint::Length(1),
                Constraint::Fill(3),
            ])
            .split(rect);

        self.sidebar = lr[0];
        self.page = lr[2];
    }

    fn focus_left(&mut self) {
        if self.page_focused {
            self.page_focused = false;
        }
    }

    fn focus_right(&mut self) {
        if !self.page_focused {
            self.page_focused = true;
        }
    }

    fn focus_up(&mut self, resources: &mut Vec<crate::resource::ResourceType>) {
        if self.page_focused {
            if let Some(rt) = self
                .sidebar_state
                .focused_index()
                .or_else(|| Some(0))
                .and_then(|id| resources.get_mut(id))
            {
                rt.cached_page_state().focus_prev();
            }
        } else {
            self.sidebar_state.focus_prev()
        }
    }

    fn focus_down(&mut self, resources: &mut Vec<crate::resource::ResourceType>) {
        if self.page_focused {
            if let Some(rt) = self
                .sidebar_state
                .focused_index()
                .or_else(|| Some(0))
                .and_then(|id| resources.get_mut(id))
            {
                rt.cached_page_state().focus_next();
            }
        } else {
            self.sidebar_state.focus_next()
        }
    }

    fn render(
        &mut self,
        frame: &mut Frame,
        resources: &mut Vec<crate::resource::ResourceType>,
        _: Option<usize>,
    ) {
        if self.sidebar.is_empty() || self.page.is_empty() {
            self.update_layout(frame.size())
        }

        self.overview(frame, resources);

        if let Some(rt) = self
            .sidebar_state
            .focused_index()
            .or_else(|| Some(0))
            .and_then(|id| resources.get_mut(id))
        {
            let mut args = PageArg {
                rect: self.page,
                active: self.page_focused,
            };
            rt.render_page(frame, &mut args);
        }
    }

    fn handle_event<'a>(&mut self, event: &super::NavigatorEvent, args: NavigatorArgs<'a>) {
        if self.page_focused {
            if let Some(rt) = self
                .sidebar_state
                .focused_index()
                .or_else(|| Some(0))
                .and_then(|id: usize| args.resources.get_mut(id))
            {
                let handled = rt.handle_navi_event(event);
                if handled {
                    return;
                }
            }
        }
        match event {
            super::NavigatorEvent::KeyEvent(key) => {
                match key.code {
                    KeyCode::Up => self.focus_up(args.resources),
                    KeyCode::Down => self.focus_down(args.resources),
                    KeyCode::Left => self.focus_left(),
                    KeyCode::Right => self.focus_right(),
                    _ => {}
                };
            }
        }
    }
}
