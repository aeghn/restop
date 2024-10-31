use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{bail, Context, Result};
use chrono::TimeDelta;
use hashbrown::{HashMap, HashSet};
use once_cell::sync::Lazy;
use process_data::{pci_slot::PciSlot, Containerization, ProcessData};
use regex::Regex;
use tracing::debug;

use crate::tarits::{NaNDefault, None2NanString};

use super::{
    process::{Process, ProcessAction, ProcessItem},
    time::boot_time,
    TICK_RATE,
};

// This contains executable names that are blacklisted from being recognized as applications
const DESKTOP_EXEC_BLOCKLIST: &[&str] = &["bash", "zsh", "fish", "sh", "ksh", "flatpak"];

// This contains IDs of desktop files that shouldn't be counted as applications for whatever reason
const APP_ID_BLOCKLIST: &[&str] = &[
    "org.gnome.Terminal.Preferences", // Prevents the actual Terminal app "org.gnome.Terminal" from being shown
    "org.freedesktop.IBus.Panel.Extension.Gtk3", // Technical application
    "org.gnome.RemoteDesktop.Handover", // Technical application
];

static RE_ENV_FILTER: Lazy<Regex> = Lazy::new(|| Regex::new(r"env\s*\S*=\S*\s*(.*)").unwrap());

static RE_FLATPAK_FILTER: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"flatpak run .* --command=(\S*)").unwrap());

// Adapted from Mission Center: https://gitlab.com/mission-center-devs/mission-center/
pub static DATA_DIRS: Lazy<Vec<PathBuf>> = Lazy::new(|| {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
    let mut data_dirs: Vec<PathBuf> = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| format!("/usr/share:{home}/.local/share"))
        .split(':')
        .map(|s| s.replace('~', &home))
        .map(PathBuf::from)
        .collect();
    data_dirs.push(PathBuf::from(format!("{home}/.local/share")));
    data_dirs
});

// This contains known occurrences of processes having a too distinct name from the actual app
// The HashMap is used like this:
//   Key: The name of the executable of the process
//   Value: What it should be replaced with when finding out to which app it belongs
static KNOWN_EXECUTABLE_NAME_EXCEPTIONS: Lazy<HashMap<String, String>> = Lazy::new(|| {
    HashMap::from([
        ("firefox-bin".into(), "firefox".into()),
        ("oosplash".into(), "libreoffice".into()),
        ("soffice.bin".into(), "libreoffice".into()),
        ("resources-processes".into(), "resources".into()),
        ("gnome-terminal-server".into(), "gnome-terminal".into()),
        ("chrome".into(), "google-chrome-stable".into()),
    ])
});

static MESSAGE_LOCALES: Lazy<Vec<String>> = Lazy::new(|| {
    let envs = ["LC_MESSAGES", "LANGUAGE", "LANG", "LC_ALL"];
    let mut return_vec: Vec<String> = Vec::new();

    for env in &envs {
        if let Ok(locales) = std::env::var(env) {
            // split because LANGUAGE may contain multiple languages
            for locale in locales.split(':') {
                let locale = locale.to_string();

                if !return_vec.contains(&locale) {
                    return_vec.push(locale.clone());
                }

                if let Some(no_character_encoding) = locale.split_once('.') {
                    let no_character_encoding = no_character_encoding.0.to_string();
                    if !return_vec.contains(&no_character_encoding) {
                        return_vec.push(no_character_encoding);
                    }
                }

                if let Some(no_country_code) = locale.split_once('_') {
                    let no_country_code = no_country_code.0.to_string();
                    if !return_vec.contains(&no_country_code) {
                        return_vec.push(no_country_code);
                    }
                }
            }
        }
    }

    debug!(
        "The following locales will be used for app names and descriptions: {:?}",
        &return_vec
    );

    return_vec
});

#[derive(Debug, Clone, Default)]
pub struct AppsContext {
    apps: HashMap<String, App>,
    processes: HashMap<i32, Process>,
    processes_assigned_to_apps: HashSet<i32>,
    read_bytes_from_dead_system_processes: u64,
    write_bytes_from_dead_system_processes: u64,
}

/// Convenience struct for displaying running applications and
/// displaying a "System Processes" item.
#[derive(Debug, Clone)]
pub struct AppItem {
    pub id: Option<String>,
    pub display_name: String,
    pub description: Option<String>,
    pub memory_usage: usize,
    pub cpu_time_ratio: f32,
    pub processes_amount: usize,
    pub containerization: Containerization,
    pub running_since: String,
    pub read_speed: f64,
    pub read_total: u64,
    pub write_speed: f64,
    pub write_total: u64,
    pub gpu_usage: f32,
    pub enc_usage: f32,
    pub dec_usage: f32,
    pub gpu_mem_usage: u64,
}

/// Represents an application installed on the system. It doesn't
/// have to be running (i.e. have alive processes).
#[derive(Debug, Clone)]
pub struct App {
    processes: Vec<i32>,
    pub commandline: Option<String>,
    pub executable_name: Option<String>,
    pub display_name: String,
    pub description: Option<String>,
    pub id: String,
    pub read_bytes_from_dead_processes: u64,
    pub write_bytes_from_dead_processes: u64,
}

impl App {
    pub fn all() -> Vec<App> {
        debug!("Detecting installed applications…");

        let apps: Vec<App> = DATA_DIRS
            .iter()
            .filter_map(|path| {
                let applications_path = path.join("applications");
                applications_path.read_dir().ok().map(|read| {
                    read.filter_map(|file_res| {
                        file_res
                            .ok()
                            .and_then(|file| Self::from_desktop_file(file.path()).ok())
                    })
                })
            })
            .flatten()
            .collect();

        debug!("Detected {} applications", apps.len());

        apps
    }

    pub fn from_desktop_file<P: AsRef<Path>>(file_path: P) -> Result<App> {
        let ini = ini::Ini::load_from_file(file_path.as_ref())?;

        let desktop_entry = ini
            .section(Some("Desktop Entry"))
            .context("no desktop entry section")?;

        let id = desktop_entry
            .get("X-Flatpak") // is there a X-Flatpak section?
            .or_else(|| desktop_entry.get("X-SnapInstanceName")) // if not, maybe there is a X-SnapInstanceName
            .map(str::to_string)
            .or_else(|| {
                // if not, presume that the ID is in the file name
                Some(
                    file_path
                        .as_ref()
                        .file_stem()?
                        .to_string_lossy()
                        .to_string(),
                )
            })
            .context("unable to get ID of desktop file")?;

        if APP_ID_BLOCKLIST.contains(&id.as_str()) {
            bail!("skipping {id} because the ID is blacklisted")
        }

        let exec = desktop_entry.get("Exec");
        let commandline = exec
            .and_then(|exec| {
                RE_ENV_FILTER
                    .captures(exec)
                    .and_then(|captures| captures.get(1))
                    .map(|capture| capture.as_str())
                    .or(Some(exec))
            })
            .map(|e| e.to_string());

        let executable_name = commandline.clone().map(|cmdline| {
            RE_FLATPAK_FILTER // filter flatpak stuff (e. g. from "/usr/bin/flatpak run … --command=inkscape …" to "inkscape")
                .captures(&cmdline)
                .and_then(|captures| captures.get(1))
                .map(|capture| capture.as_str().to_string())
                .unwrap_or(cmdline) // if there's no flatpak stuff, return the bare cmdline
                .split(' ') // filter any arguments (e. g. from "/usr/bin/firefox %u" to "/usr/bin/firefox")
                .nth(0)
                .unwrap_or_default()
                .split('/') // filter the executable path (e. g. from "/usr/bin/firefox" to "firefox")
                .nth_back(0)
                .unwrap_or_default()
                .to_string()
        });

        if let Some(executable_name) = &executable_name {
            if DESKTOP_EXEC_BLOCKLIST.contains(&executable_name.as_str()) {
                bail!(
                    "skipping {} because its executable {} is blacklisted",
                    id,
                    executable_name
                )
            }
        }

        let mut display_name_opt = None;
        let mut description_opt = None;

        for locale in MESSAGE_LOCALES.iter() {
            if let Some(name) = desktop_entry.get(format!("Name[{locale}]")) {
                display_name_opt = Some(name);
                break;
            }
        }

        for locale in MESSAGE_LOCALES.iter() {
            if let Some(comment) = desktop_entry.get(format!("Comment[{locale}]")) {
                description_opt = Some(comment);
                break;
            }
        }

        let display_name = display_name_opt
            .or_else(|| desktop_entry.get("Name"))
            .unwrap_or(&id)
            .to_string();

        let description = description_opt
            .or_else(|| desktop_entry.get("Comment"))
            .map(str::to_string);

        Ok(App {
            processes: Vec::new(),
            commandline,
            executable_name,
            display_name,
            description,
            id,
            read_bytes_from_dead_processes: 0,
            write_bytes_from_dead_processes: 0,
        })
    }

    /// Adds a process to the processes `HashMap` and also
    /// updates the `Process`' icon to the one of this
    /// `App`
    pub fn add_process(&mut self, process: &mut Process) {
        self.processes.push(process.data.pid);
    }

    pub fn remove_process(&mut self, process: &Process) {
        self.processes.retain(|p| *p != process.data.pid);
    }

    #[must_use]
    pub fn is_running(&self) -> bool {
        !self.processes.is_empty()
    }

    pub fn processes_iter<'a>(&'a self, apps: &'a AppsContext) -> impl Iterator<Item = &Process> {
        apps.all_processes()
            .filter(move |process| self.processes.contains(&process.data.pid))
    }

    pub fn processes_iter_mut<'a>(
        &'a mut self,
        apps: &'a mut AppsContext,
    ) -> impl Iterator<Item = &mut Process> {
        apps.all_processes_mut()
            .filter(move |process| self.processes.contains(&process.data.pid))
    }

    #[must_use]
    pub fn memory_usage(&self, apps: &AppsContext) -> usize {
        self.processes_iter(apps)
            .map(|process| process.data.memory_usage)
            .sum()
    }

    #[must_use]
    pub fn cpu_time_ratio(&self, apps: &AppsContext) -> f32 {
        self.processes_iter(apps).map(Process::cpu_time_ratio).sum()
    }

    #[must_use]
    pub fn read_speed(&self, apps: &AppsContext) -> f64 {
        self.processes_iter(apps)
            .filter_map(Process::read_speed)
            .sum()
    }

    #[must_use]
    pub fn read_total(&self, apps: &AppsContext) -> u64 {
        self.read_bytes_from_dead_processes.saturating_add(
            self.processes_iter(apps)
                .filter_map(|process| process.data.read_bytes)
                .sum::<u64>(),
        )
    }

    #[must_use]
    pub fn write_speed(&self, apps: &AppsContext) -> f64 {
        self.processes_iter(apps)
            .filter_map(Process::write_speed)
            .sum()
    }

    #[must_use]
    pub fn write_total(&self, apps: &AppsContext) -> u64 {
        self.write_bytes_from_dead_processes.saturating_add(
            self.processes_iter(apps)
                .filter_map(|process| process.data.write_bytes)
                .sum::<u64>(),
        )
    }

    #[must_use]
    pub fn gpu_usage(&self, apps: &AppsContext) -> f32 {
        self.processes_iter(apps).map(Process::gpu_usage).sum()
    }

    #[must_use]
    pub fn enc_usage(&self, apps: &AppsContext) -> f32 {
        self.processes_iter(apps).map(Process::enc_usage).sum()
    }

    #[must_use]
    pub fn dec_usage(&self, apps: &AppsContext) -> f32 {
        self.processes_iter(apps).map(Process::dec_usage).sum()
    }

    #[must_use]
    pub fn gpu_mem_usage(&self, apps: &AppsContext) -> u64 {
        self.processes_iter(apps).map(Process::gpu_mem_usage).sum()
    }

    #[must_use]
    pub fn starttime(&self, apps: &AppsContext) -> f64 {
        self.processes_iter(apps)
            .map(Process::starttime)
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or_default()
    }

    pub fn execute_process_action(
        &self,
        apps: &AppsContext,
        action: ProcessAction,
    ) -> Vec<Result<()>> {
        self.processes_iter(apps)
            .map(|process| process.execute_process_action(action))
            .collect()
    }
}

impl AppsContext {
    /// Creates a new `AppsContext` object, this operation is quite expensive
    /// so try to do it only one time during the lifetime of the program.
    /// Please call refresh() immediately after this function.
    pub fn new() -> AppsContext {
        let apps: HashMap<String, App> = App::all()
            .into_iter()
            .map(|app| (app.id.clone(), app))
            .collect();

        AppsContext {
            apps,
            processes: HashMap::new(),
            processes_assigned_to_apps: HashSet::new(),
            read_bytes_from_dead_system_processes: 0,
            write_bytes_from_dead_system_processes: 0,
        }
    }

    pub fn gpu_fraction(&self, pci_slot: PciSlot) -> f32 {
        self.all_processes()
            .map(|process| {
                (
                    &process.data.gpu_usage_stats,
                    &process.gpu_usage_stats_last,
                    process.data.timestamp,
                    process.timestamp_last,
                )
            })
            .map(|(new, old, timestamp, timestamp_last)| {
                (
                    new.get(&pci_slot),
                    old.get(&pci_slot),
                    timestamp,
                    timestamp_last,
                )
            })
            .filter_map(|(new, old, timestamp, timestamp_last)| match (new, old) {
                (Some(new), Some(old)) => Some((new, old, timestamp, timestamp_last)),
                _ => None,
            })
            .map(|(new, old, timestamp, timestamp_last)| {
                if new.nvidia {
                    new.gfx as f32 / 100.0
                } else if old.gfx == 0 {
                    0.0
                } else {
                    ((new.gfx.saturating_sub(old.gfx) as f32)
                        / (timestamp.saturating_sub(timestamp_last) as f32))
                        .nan_default(0.0)
                        / 1_000_000.0
                }
            })
            .sum()
    }

    pub fn encoder_fraction(&self, pci_slot: PciSlot) -> f32 {
        self.all_processes()
            .map(|process| {
                (
                    &process.data.gpu_usage_stats,
                    &process.gpu_usage_stats_last,
                    process.data.timestamp,
                    process.timestamp_last,
                )
            })
            .map(|(new, old, timestamp, timestamp_last)| {
                (
                    new.get(&pci_slot),
                    old.get(&pci_slot),
                    timestamp,
                    timestamp_last,
                )
            })
            .filter_map(|(new, old, timestamp, timestamp_last)| match (new, old) {
                (Some(new), Some(old)) => Some((new, old, timestamp, timestamp_last)),
                _ => None,
            })
            .map(|(new, old, timestamp, timestamp_last)| {
                if new.nvidia {
                    new.enc as f32 / 100.0
                } else if old.enc == 0 {
                    0.0
                } else {
                    ((new.enc.saturating_sub(old.enc) as f32)
                        / (timestamp.saturating_sub(timestamp_last) as f32))
                        .nan_default(0.0)
                        / 1_000_000.0
                }
            })
            .sum()
    }

    pub fn decoder_fraction(&self, pci_slot: PciSlot) -> f32 {
        self.all_processes()
            .map(|process| {
                (
                    &process.data.gpu_usage_stats,
                    &process.gpu_usage_stats_last,
                    process.data.timestamp,
                    process.timestamp_last,
                )
            })
            .map(|(new, old, timestamp, timestamp_last)| {
                (
                    new.get(&pci_slot),
                    old.get(&pci_slot),
                    timestamp,
                    timestamp_last,
                )
            })
            .filter_map(|(new, old, timestamp, timestamp_last)| match (new, old) {
                (Some(new), Some(old)) => Some((new, old, timestamp, timestamp_last)),
                _ => None,
            })
            .map(|(new, old, timestamp, timestamp_last)| {
                if new.nvidia {
                    new.dec as f32 / 100.0
                } else if old.dec == 0 {
                    0.0
                } else {
                    ((new.dec.saturating_sub(old.dec) as f32)
                        / (timestamp.saturating_sub(timestamp_last) as f32))
                        .nan_default(0.0)
                        / 1_000_000.0
                }
            })
            .sum()
    }

    fn app_associated_with_process(&self, process: &Process) -> Option<String> {
        // TODO: tidy this up
        // ↓ look for whether we can find an ID in the cgroup
        if let Some(app) = self
            .apps
            .get(process.data.cgroup.as_deref().unwrap_or_default())
        {
            Some(app.id.clone())
        } else if let Some(app) = self.apps.get(&process.executable_path) {
            // ↑ look for whether we can find an ID in the executable path of the process
            Some(app.id.clone())
        } else if let Some(app) = self.apps.get(&process.executable_name) {
            // ↑ look for whether we can find an ID in the executable name of the process
            Some(app.id.clone())
        } else {
            self.apps
                .values()
                .find(|app| {
                    // ↓ probably most expensive lookup, therefore only last resort: look if the process' commandline
                    //   can be found in the app's commandline
                    let commandline_match = app
                        .commandline
                        .as_ref()
                        .is_some_and(|commandline| commandline == &process.executable_path);

                    let executable_name_match = app
                        .executable_name
                        .as_ref()
                        .is_some_and(|executable_name| executable_name == &process.executable_name);

                    let known_exception_match = app
                        .executable_name
                        .as_ref()
                        .and_then(|executable_name| {
                            KNOWN_EXECUTABLE_NAME_EXCEPTIONS
                                .get(&process.executable_name)
                                .map(|substituted_executable_name| {
                                    substituted_executable_name == executable_name
                                })
                        })
                        .unwrap_or(false);

                    commandline_match || executable_name_match || known_exception_match
                })
                .map(|app| app.id.clone())
        }
    }

    pub fn get_process(&self, pid: i32) -> Option<&Process> {
        self.processes.get(&pid)
    }

    pub fn get_app(&self, id: &str) -> Option<&App> {
        self.apps.get(id)
    }

    #[must_use]
    pub fn all_processes(&self) -> impl Iterator<Item = &Process> {
        self.processes.values()
    }

    #[must_use]
    pub fn all_processes_mut(&mut self) -> impl Iterator<Item = &mut Process> {
        self.processes.values_mut()
    }

    /// Returns a `HashMap` of running processes. For more info, refer to
    /// `ProcessItem`.
    pub fn process_items(&self) -> HashMap<i32, ProcessItem> {
        self.all_processes()
            .map(|process| (process.data.pid, self.process_item(process.data.pid)))
            .filter_map(|(pid, process_opt)| process_opt.map(|process| (pid, process)))
            .collect()
    }

    pub fn process_item(&self, pid: i32) -> Option<ProcessItem> {
        self.get_process(pid).map(|process| {
            let full_comm = if process.executable_name.starts_with(&process.data.comm) {
                process.executable_name.clone()
            } else {
                process.data.comm.clone()
            };
            ProcessItem {
                pid: process.data.pid,
                user: process.data.user.clone(),
                display_name: full_comm.clone(),
                memory_usage: process.data.memory_usage,
                cpu_time_ratio: process.cpu_time_ratio(),
                user_cpu_time: ((process.data.user_cpu_time) as f64 / (*TICK_RATE) as f64),
                system_cpu_time: ((process.data.system_cpu_time) as f64 / (*TICK_RATE) as f64),
                commandline: Process::sanitize_cmdline(process.data.commandline.clone())
                    .unwrap_or(full_comm),
                containerization: process.data.containerization,
                starttime: process.starttime(),
                cgroup: process.data.cgroup.clone(),
                read_speed: process.read_speed(),
                read_total: process.data.read_bytes,
                write_speed: process.write_speed(),
                write_total: process.data.write_bytes,
                gpu_usage: process.gpu_usage(),
                enc_usage: process.enc_usage(),
                dec_usage: process.dec_usage(),
                gpu_mem_usage: process.gpu_mem_usage(),
            }
        })
    }

    /// Returns a `HashMap` of running graphical applications. For more info,
    /// refer to `AppItem`.
    #[must_use]
    pub fn app_items(&self) -> HashMap<Option<String>, AppItem> {
        let mut app_pids = HashSet::new();

        let mut return_map = self
            .apps
            .iter()
            .filter(|(_, app)| app.is_running() && !app.id.starts_with("xdg-desktop-portal"))
            .map(|(_, app)| {
                app.processes_iter(self).for_each(|process| {
                    app_pids.insert(process.data.pid);
                });

                let is_flatpak = app
                    .processes_iter(self)
                    .filter(|process| {
                        !process.data.commandline.starts_with("bwrap")
                            && !process.data.commandline.is_empty()
                    })
                    .any(|process| process.data.containerization == Containerization::Flatpak);

                let is_snap = app
                    .processes_iter(self)
                    .filter(|process| {
                        !process.data.commandline.starts_with("bwrap")
                            && !process.data.commandline.is_empty()
                    })
                    .any(|process| process.data.containerization == Containerization::Snap);

                let containerization = if is_flatpak {
                    Containerization::Flatpak
                } else if is_snap {
                    Containerization::Snap
                } else {
                    Containerization::None
                };

                let running_since = boot_time()
                    .and_then(|boot_time| {
                        boot_time
                            .checked_add_signed(
                                TimeDelta::from_std(
                                    Duration::from_secs(app.starttime(self) as u64),
                                )
                                .unwrap(),
                            )
                            .context("unable to add seconds to boot time")
                    })
                    .and_then(|time| Ok(time.to_string()))
                    .ok()
                    .or_nan_owned();

                (
                    Some(app.id.clone()),
                    AppItem {
                        id: Some(app.id.clone()),
                        display_name: app.display_name.clone(),
                        description: app.description.clone(),
                        memory_usage: app.memory_usage(self),
                        cpu_time_ratio: app.cpu_time_ratio(self),
                        processes_amount: app.processes_iter(self).count(),
                        containerization,
                        running_since,
                        read_speed: app.read_speed(self),
                        read_total: app.read_total(self),
                        write_speed: app.write_speed(self),
                        write_total: app.write_total(self),
                        gpu_usage: app.gpu_usage(self),
                        enc_usage: app.enc_usage(self),
                        dec_usage: app.dec_usage(self),
                        gpu_mem_usage: app.gpu_mem_usage(self),
                    },
                )
            })
            .collect::<HashMap<Option<String>, AppItem>>();

        let system_cpu_ratio = self
            .system_processes_iter()
            .map(Process::cpu_time_ratio)
            .sum();

        let system_memory_usage: usize = self
            .system_processes_iter()
            .map(|process| process.data.memory_usage)
            .sum();

        let system_read_speed = self
            .system_processes_iter()
            .filter_map(Process::read_speed)
            .sum();

        let system_read_total = self.read_bytes_from_dead_system_processes
            + self
                .system_processes_iter()
                .filter_map(|process| process.data.read_bytes)
                .sum::<u64>();

        let system_write_speed = self
            .system_processes_iter()
            .filter_map(Process::write_speed)
            .sum();

        let system_write_total = self.write_bytes_from_dead_system_processes
            + self
                .system_processes_iter()
                .filter_map(|process| process.data.write_bytes)
                .sum::<u64>();

        let system_gpu_usage = self.system_processes_iter().map(Process::gpu_usage).sum();

        let system_enc_usage = self.system_processes_iter().map(Process::enc_usage).sum();

        let system_dec_usage = self.system_processes_iter().map(Process::dec_usage).sum();

        let system_gpu_mem_usage = self
            .system_processes_iter()
            .map(Process::gpu_mem_usage)
            .sum();

        let system_running_since = boot_time()
            .and_then(|boot_time| Ok(boot_time.to_string()))
            .ok()
            .or_nan_owned();

        return_map.insert(
            None,
            AppItem {
                id: None,
                display_name: String::from("System Processes"),
                description: None,
                memory_usage: system_memory_usage,
                cpu_time_ratio: system_cpu_ratio,
                processes_amount: self
                    .processes
                    .len()
                    .saturating_sub(self.processes_assigned_to_apps.len()),
                containerization: Containerization::None,
                running_since: system_running_since,
                read_speed: system_read_speed,
                read_total: system_read_total,
                write_speed: system_write_speed,
                write_total: system_write_total,
                gpu_usage: system_gpu_usage,
                enc_usage: system_enc_usage,
                dec_usage: system_dec_usage,
                gpu_mem_usage: system_gpu_mem_usage,
            },
        );

        return_map
    }

    /// Refreshes the statistics about the running applications and processes.
    pub fn refresh(&mut self, new_process_data: Vec<ProcessData>) {
        let mut updated_processes = HashSet::new();

        for process_data in new_process_data {
            updated_processes.insert(process_data.pid);
            // refresh our old processes
            if let Some(old_process) = self.processes.get_mut(&process_data.pid) {
                old_process.cpu_time_last = old_process
                    .data
                    .user_cpu_time
                    .saturating_add(old_process.data.system_cpu_time);
                old_process.timestamp_last = old_process.data.timestamp;
                old_process.read_bytes_last = old_process.data.read_bytes;
                old_process.write_bytes_last = old_process.data.write_bytes;
                old_process.gpu_usage_stats_last = old_process.data.gpu_usage_stats.clone();

                old_process.data = process_data.clone();
            } else {
                // this is a new process, see if it belongs to a graphical app

                let mut new_process = Process::from_process_data(process_data);

                if let Some(app_id) = self.app_associated_with_process(&new_process) {
                    self.processes_assigned_to_apps.insert(new_process.data.pid);
                    self.apps
                        .get_mut(&app_id)
                        .unwrap()
                        .add_process(&mut new_process);
                }

                self.processes.insert(new_process.data.pid, new_process);
            }
        }

        // all the not-updated processes have unfortunately died, probably

        // collect the I/O stats for died app processes so an app doesn't suddenly have less total disk I/O
        self.apps.values_mut().for_each(|app| {
            let (read_dead, write_dead) = app
                .processes
                .iter()
                .filter(|pid| !updated_processes.contains(*pid)) // only dead processes
                .filter_map(|pid| self.processes.get(pid)) // ignore about non-existing processes
                .map(|process| (process.data.read_bytes, process.data.write_bytes)) // get their read_bytes and write_bytes
                .filter_map(
                    // filter out any processes whose IO stats we were not allowed to see
                    |(read_bytes, write_bytes)| match (read_bytes, write_bytes) {
                        (Some(read), Some(write)) => Some((read, write)),
                        _ => None,
                    },
                )
                .reduce(|sum, current| (sum.0 + current.0, sum.1 + current.1)) // sum them up
                .unwrap_or((0, 0)); // if there were no processes, it's 0 for both

            app.read_bytes_from_dead_processes += read_dead;
            app.write_bytes_from_dead_processes += write_dead;

            app.processes.retain(|pid| updated_processes.contains(pid));

            if !app.is_running() {
                app.read_bytes_from_dead_processes = 0;
                app.write_bytes_from_dead_processes = 0;
            }
        });

        // same as above but for system processes
        let (read_dead, write_dead) = self
            .processes
            .iter()
            .filter(|(pid, _)| {
                !self.processes_assigned_to_apps.contains(*pid) && !updated_processes.contains(*pid)
            })
            .map(|(_, process)| (process.data.read_bytes, process.data.write_bytes))
            .filter_map(
                |(read_bytes, write_bytes)| match (read_bytes, write_bytes) {
                    (Some(read), Some(write)) => Some((read, write)),
                    _ => None,
                },
            )
            .reduce(|sum, current| (sum.0 + current.0, sum.1 + current.1))
            .unwrap_or((0, 0));
        self.read_bytes_from_dead_system_processes += read_dead;
        self.write_bytes_from_dead_system_processes += write_dead;

        // remove the dead process from our process map
        self.processes
            .retain(|pid, _| updated_processes.contains(pid));

        // remove the dead process from out list of app processes
        self.processes_assigned_to_apps
            .retain(|pid| updated_processes.contains(pid));
    }

    pub fn system_processes_iter(&self) -> impl Iterator<Item = &Process> {
        self.all_processes()
            .filter(|process| !self.processes_assigned_to_apps.contains(&process.data.pid))
    }
}
