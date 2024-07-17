use std::{cell::Cell, sync::Arc, thread};

use chin_tools::wrapper::anyhow::AResult;
use flume::{Receiver, Sender};

use itertools::Itertools;
use once_cell::sync::Lazy;
use process_data::ProcessData;
use ratatui::{
    style::{Color, Style, Stylize},
    text::{Line, Span},
};
use unicode_width::UnicodeWidthChar;

pub const PROCESS_ID: &'static str = "PROCESS";

static PROCESS_WORKER_CHANNEL: Lazy<(Sender<ProcessMsg>, Receiver<ProcessMsg>)> =
    Lazy::new(|| flume::unbounded());

use crate::{
    app::ResourceEvent,
    component::{
        grouped_lines::GroupedLines,
        s_label,
        stateful_lines::{StatefulColumn, StatefulLinesType},
    },
    sensor::{
        apps::AppsContext,
        process::{read_proc_loadavg, read_proc_uptime, LoadAvg, ProcessItem},
        units::{conver_storage_width4, convert_seconds},
    },
    tarits::{None2NaN, None2NanString},
    view::theme::SharedTheme,
    view::{OverviewArg, PageArg},
};

use super::{Resource, SensorResultType};

#[derive(Debug)]
pub struct ResProcess {
    data: Option<Arc<Vec<ProcessItem>>>,
    loadavg: Option<LoadAvg>,
    uptime: Cell<u64>,
    theme: SharedTheme,

    view_state: StatefulColumn<'static>,
    line_builder: LineBuilder,
}

#[derive(Debug)]
pub enum ProcessRsp {
    Processes(Arc<Vec<ProcessItem>>),
    LoadAvg(LoadAvg),
    Uptime(u64),
}

impl ResProcess {
    pub fn spawn(theme: SharedTheme, result_tx: &Sender<ResourceEvent>) -> AResult<Self> {
        ProcessWorker::spawn(result_tx)?;
        Ok(Self {
            data: None,
            theme,
            loadavg: Default::default(),
            uptime: Default::default(),
            view_state: StatefulColumn::new(),
            line_builder: LineBuilder::new(),
        })
    }
}

impl Resource for ResProcess {
    type Req = ();

    type Rsp = ProcessRsp;

    fn do_sensor(_: Self::Req) -> AResult<SensorResultType> {
        PROCESS_WORKER_CHANNEL.0.send(ProcessMsg::Detect)?;
        Ok(SensorResultType::AsyncResult)
    }

    fn get_id(&self) -> &str {
        PROCESS_ID
    }

    fn get_req(&self) -> Self::Req {
        ()
    }

    fn overview_content(&self, args: &mut OverviewArg) -> AResult<GroupedLines<'static>> {
        let width = args.width;
        let block = GroupedLines::builder(width, &self.theme)
            .kv("Uptime", convert_seconds(self.uptime.get()))
            .kv("Load", self.loadavg.or_nan_owned())
            .kv(
                "Processes",
                self.data.or_nan(|e| {
                    format!(
                        "{} {}",
                        e.len(),
                        self.loadavg
                            .as_ref()
                            .map_or("".to_string(), |e| e.processes.to_string())
                    )
                }),
            )
            .active(args.focused)
            .build("Process")?;

        Ok(block)
    }

    fn _build_page(&mut self, args: &mut PageArg) -> AResult<String> {
        self.view_state.set_header(self.line_builder.to_header());
        self.view_state.update_view_height(args.rect.height);
        if let Some(data) = self.data.as_ref() {
            self.view_state
                .update_lines(data, |e, s| self.line_builder.to_line(e, s));
        }

        Ok("Process".to_string())
    }

    fn update_data(&mut self, data: &Self::Rsp) {
        match data {
            ProcessRsp::Processes(process_data) => {
                self.data.replace(process_data.clone());
            }
            ProcessRsp::LoadAvg(load) => {
                self.loadavg.replace(load.clone());
            }
            ProcessRsp::Uptime(uptime) => {
                self.uptime.set(*uptime);
            }
        }
    }

    fn cached_page_state<'b>(&'b mut self) -> StatefulLinesType<'static, 'b> {
        StatefulLinesType::Lines(&mut self.view_state)
    }

    fn get_type_name(&self) -> &'static str {
        PROCESS_ID
    }

    fn handle_page_event(&mut self, _event: &crossterm::event::Event) {}

    fn get_name(&self) -> String {
        "".to_string()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(dead_code)]
enum ProcessCell {
    PID,
    PRG,
    USER,
    CMD,
    MEM,
    CPU,
    READ,
    WRITE,
    TIME,
}

impl ProcessCell {
    fn width(&self) -> u16 {
        match self {
            ProcessCell::PID => 9,
            ProcessCell::PRG => 11,
            ProcessCell::USER => 6,
            ProcessCell::CMD => 200,
            ProcessCell::MEM => 7,
            ProcessCell::CPU => 6,
            ProcessCell::READ => 7,
            ProcessCell::WRITE => 7,
            ProcessCell::TIME => 20,
        }
    }

    fn keep_width<S>(&self, noodle: S) -> String
    where
        S: Into<String>,
    {
        let total_width = self.width();
        let content_width = total_width.saturating_sub(2);
        let mut noodle: String = noodle.into();

        let mut width: usize = 0;
        let mut size = None;

        for c in noodle.chars() {
            if let Some(wid) = c.width() {
                let nw = width.saturating_add(wid);
                if nw > content_width.into() {
                    size.replace(width);
                    break;
                }
                width = nw;
            }
        }

        if let Some(size) = size {
            noodle.truncate(size);
        }

        let padding = total_width.saturating_sub(width as u16);
        if padding > 0 {
            for _ in 0..padding {
                noodle.push(' ');
            }
        }

        noodle
    }

    fn to_value(&self, data: &ProcessItem) -> Span<'static> {
        let s = match self {
            ProcessCell::PID => self.keep_width(data.pid.to_string().as_str()),
            ProcessCell::USER => self.keep_width(data.user.as_str()),
            ProcessCell::CMD => self.keep_width(data.commandline.as_str()),
            ProcessCell::MEM => {
                self.keep_width(conver_storage_width4(data.memory_usage as f64).as_str())
            }
            ProcessCell::CPU => {
                self.keep_width(format!("{:.1}", data.cpu_time_ratio * 100.).as_str())
            }
            ProcessCell::READ => match data.read_speed.as_ref() {
                Some(o) => self.keep_width(conver_storage_width4(*o).as_str()),
                None => {
                    return s_label(
                        &self.keep_width(conver_storage_width4(0.).as_str()),
                        Style::new().fg(Color::DarkGray),
                    )
                }
            },
            ProcessCell::WRITE => match data.write_speed.as_ref() {
                Some(o) => self.keep_width(conver_storage_width4(*o).as_str()),
                None => {
                    return s_label(
                        &self.keep_width(conver_storage_width4(0.).as_str()),
                        Style::new().fg(Color::DarkGray),
                    )
                }
            },
            ProcessCell::TIME => self.keep_width(data.starttime.to_string().as_str()),
            ProcessCell::PRG => self.keep_width(&data.display_name),
        };

        s.into()
    }

    fn to_label(&self, suffix: char) -> Span<'static> {
        let mut s = String::new();
        let label = match self {
            ProcessCell::PID => "PID",
            ProcessCell::PRG => "NAME",
            ProcessCell::USER => "USER",
            ProcessCell::CMD => "CMD",
            ProcessCell::MEM => "MEM",
            ProcessCell::CPU => "CPU",
            ProcessCell::READ => "READ",
            ProcessCell::WRITE => "WRIT",
            ProcessCell::TIME => "TIME",
        };
        s.push_str(label);
        s.push(suffix);

        s = self.keep_width(s);
        s.into()
    }
}

#[derive(Debug)]
struct LineBuilder {
    labels: Vec<ProcessCell>,
    sort_filed: ProcessCell,
    sort_asc: bool,
}

impl LineBuilder {
    pub fn new() -> Self {
        let header = vec![
            ProcessCell::PID,
            ProcessCell::USER,
            ProcessCell::CPU,
            ProcessCell::MEM,
            ProcessCell::READ,
            ProcessCell::WRITE,
            ProcessCell::CMD,
        ];

        let mut viewer = Self {
            labels: header,
            sort_filed: ProcessCell::CPU,
            sort_asc: false,
        };

        viewer.set_sort(ProcessCell::CPU, false);

        viewer
    }

    pub fn set_sort(&mut self, field: ProcessCell, sort_asc: bool) {
        self.sort_asc = sort_asc;
        self.sort_filed = field;
    }

    fn to_line(&self, item: &ProcessItem, active: bool) -> Line<'static> {
        let spans: Vec<Span<'static>> = self.labels.iter().map(|pc| pc.to_value(item)).collect();
        let line: Line<'static> = spans.into();
        if active {
            line.bg(Color::DarkGray)
        } else {
            line
        }
    }

    fn to_header(&self) -> Line<'static> {
        let mut spans: Vec<Span<'static>> = vec![];
        for ele in self.labels.iter() {
            spans.push(ele.to_label(' '))
        }
        spans.into()
    }
}

pub enum ProcessSortType {
    PID(bool),
    Name(bool),
    CPU(bool),
    MEM(bool),
}

pub enum ProcessMsg {
    Sort(ProcessSortType),
    Detect,
}

pub struct Process {}

pub struct ProcessWorker {
    sort_type: Option<ProcessSortType>,
    app_context: AppsContext,
}

impl ProcessWorker {
    pub fn spawn(result_tx: &Sender<ResourceEvent>) -> AResult<()> {
        let mut worker = ProcessWorker {
            sort_type: None,
            app_context: AppsContext::new(),
        };

        let req_rx = PROCESS_WORKER_CHANNEL.1.clone();
        let result_tx = result_tx.clone();

        thread::Builder::new()
            .name("processworker".to_owned())
            .spawn(move || loop {
                if let Ok(msg) = req_rx.recv() {
                    match msg {
                        ProcessMsg::Sort(sort) => {
                            worker.sort_type.replace(sort);
                        }
                        ProcessMsg::Detect => {
                            if let Ok(uptime) = read_proc_uptime() {
                                let _ = result_tx.send(ResourceEvent::SensorRsp(
                                    crate::resource::SensorRsp::Process(ProcessRsp::Uptime(uptime)),
                                ));
                            }
                            if let Ok(load) = read_proc_loadavg() {
                                let _ = result_tx.send(ResourceEvent::SensorRsp(
                                    crate::resource::SensorRsp::Process(ProcessRsp::LoadAvg(load)),
                                ));
                            }
                            worker.updata_data();
                            let _ = result_tx.send(ResourceEvent::SensorRsp(
                                crate::resource::SensorRsp::Process(ProcessRsp::Processes(
                                    Arc::new(worker.get_process_items()),
                                )),
                            ));
                        }
                    }
                }
            })?;

        Ok(())
    }

    pub fn updata_data(&mut self) {
        match ProcessData::all_process_data() {
            Ok(data) => {
                self.app_context.refresh(data);
            }
            Err(err) => {
                tracing::error!("unable to update process data: {}", err);
            }
        }
    }

    pub fn get_process_items(&self) -> Vec<ProcessItem> {
        self.app_context
            .process_items()
            .into_iter()
            .map(|(_, v)| v)
            .sorted_by(|e1, e2| e2.cpu_time_ratio.total_cmp(&e1.cpu_time_ratio))
            .collect()
    }
}
