use std::{
    fmt::Display,
    path::{Path, PathBuf},
    str::{self, FromStr},
};

use anyhow::{bail, Context, Result};

use crate::tarits::{None2NaN, None2NanString};

use super::{units::convert_energy, Sensor};

#[derive(Debug)]
pub struct BatteryData {
    pub inner: Battery,
    pub charge: Result<f64>,
    pub power_usage: Result<f64>,
    pub health: Result<f64>,
    pub state: Result<State>,
    pub charge_cycles: Result<usize>,
}

impl BatteryData {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        let inner = Battery::from_sysfs(path.as_ref());
        let charge = inner.charge();
        let power_usage = inner.power_usage();
        let health = inner.health();
        let state = inner.state();
        let charge_cycles = inner.charge_cycles();

        Self {
            inner,
            charge,
            power_usage,
            health,
            state,
            charge_cycles,
        }
    }
}

#[derive(Debug, Eq, PartialEq, Copy, Clone, Default)]
pub enum State {
    Charging,
    Discharging,
    Empty,
    Full,
    #[default]
    Unknown,
}

impl str::FromStr for State {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let state = match s.to_ascii_lowercase().as_str() {
            "charging" => State::Charging,
            "discharging" => State::Discharging,
            "empty" => State::Empty,
            "full" => State::Full,
            _ => State::Unknown,
        };

        Ok(state)
    }
}

impl Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                State::Charging => "Charging",
                State::Discharging => "Discharging",
                State::Empty => "Empty",
                State::Full => "Full",
                State::Unknown => "Unknown",
            }
        )
    }
}

#[derive(Debug, Eq, PartialEq, Copy, Clone, Default)]
pub enum Technology {
    NickelMetalHydride,
    NickelCadmium,
    NickelZinc,
    LeadAcid,
    LithiumIon,
    LithiumIronPhosphate,
    LithiumPolymer,
    RechargeableAlkalineManganese,
    #[default]
    Unknown,
}

impl str::FromStr for Technology {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let tech = match s.to_ascii_lowercase().as_str() {
            "nimh" => Technology::NickelMetalHydride,
            "nicd" => Technology::NickelCadmium,
            "nizn" => Technology::NickelZinc,
            "pb" => Technology::LeadAcid,
            "pbac" => Technology::LeadAcid,
            "li-i" => Technology::LithiumIon,
            "li-ion" => Technology::LithiumIon,
            "lion" => Technology::LithiumIon,
            "life" => Technology::LithiumIronPhosphate,
            "lip" => Technology::LithiumPolymer,
            "lipo" => Technology::LithiumPolymer,
            "li-poly" => Technology::LithiumPolymer,
            "ram" => Technology::RechargeableAlkalineManganese,
            _ => Technology::Unknown,
        };

        Ok(tech)
    }
}

impl Display for Technology {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Technology::NickelMetalHydride => "Nickel-Metal Hydride",
                Technology::NickelCadmium => "Nickel-Cadmium",
                Technology::NickelZinc => "Nickel-Zinc",
                Technology::LeadAcid => "Lead-Acid",
                Technology::LithiumIon => "Lithium-Ion",
                Technology::LithiumIronPhosphate => "Lithium Iron Phosphate",
                Technology::LithiumPolymer => "Lithium Polymer",
                Technology::RechargeableAlkalineManganese => "Rechargeable Alkaline Managanese",
                Technology::Unknown => "N/A",
            }
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Battery {
    pub supply_name: String,
    pub sysfs_path: PathBuf,
    pub manufacturer: Option<String>,
    pub model_name: Option<String>,
    pub design_capacity: Option<f64>,
    pub technology: Technology,
}

impl Battery {
    pub fn get_sysfs_paths() -> Result<Vec<PathBuf>> {
        let mut list = Vec::new();
        let mut entries = std::fs::read_dir("/sys/class/power_supply")?;
        while let Some(entry) = entries.next() {
            let entry = entry?;
            if std::fs::read_to_string(entry.path().join("type"))
                .unwrap_or_default()
                .to_ascii_lowercase()
                .trim()
                != "battery"
            {
                continue;
            }
            list.push(entry.path());
        }
        Ok(list)
    }

    pub fn from_sysfs<P: AsRef<Path>>(sysfs_path: P) -> Battery {
        let sysfs_path = sysfs_path.as_ref().to_path_buf();
        let file_name = sysfs_path
            .file_name()
            .unwrap()
            .to_str()
            .or_unk(|e| e.to_string());

        let manufacturer = std::fs::read_to_string(sysfs_path.join("manufacturer"))
            .map(|s| s.replace('\n', ""))
            .ok();

        let model_name = std::fs::read_to_string(sysfs_path.join("model_name"))
            .map(|s| s.replace('\n', ""))
            .ok();

        let technology = Technology::from_str(
            &std::fs::read_to_string(sysfs_path.join("technology"))
                .map(|s| s.replace('\n', ""))
                .unwrap_or_default(),
        )
        .unwrap_or_default();

        let design_capacity = std::fs::read_to_string(sysfs_path.join("energy_full_design"))
            .context("unable to find any energy_full_design")
            .and_then(|capacity| {
                capacity
                    .trim()
                    .parse::<usize>()
                    .map(|int| int as f64 / 1_000_000.0)
                    .context("can't parse energy_full_design")
            })
            .ok();

        Battery {
            supply_name: file_name,
            sysfs_path,
            manufacturer,
            model_name,
            design_capacity,
            technology,
        }
    }

    pub fn display_name(&self) -> String {
        if let Some(design_capacity) = self.design_capacity {
            let converted_energy = convert_energy(design_capacity, true);
            format!("{} Battery", &converted_energy)
        } else {
            String::from("Battery")
        }
    }

    pub fn charge(&self) -> Result<f64> {
        std::fs::read_to_string(self.sysfs_path.join("capacity"))?
            .trim()
            .parse::<u8>()
            .map(|percent| percent as f64 / 100.0)
            .context("unable to parse capacity sysfs file")
    }

    pub fn health(&self) -> Result<f64> {
        let energy_full = std::fs::read_to_string(self.sysfs_path.join("energy_full"))
            .context("unable to read energy_full sysfs file")
            .and_then(|x| {
                x.trim()
                    .parse::<usize>()
                    .context("unable to parse energiy_full sysfs file")
            });

        let energy_full_design =
            std::fs::read_to_string(self.sysfs_path.join("energy_full_design"))
                .context("unable to read energy_full_design sysfs file")
                .and_then(|x| {
                    x.trim()
                        .parse::<usize>()
                        .context("unable to parse energy_full_design sysfs file")
                });

        if let (Ok(energy_full), Ok(energy_full_design)) = (energy_full, energy_full_design) {
            Ok(energy_full as f64 / energy_full_design as f64)
        } else {
            let charge_full = std::fs::read_to_string(self.sysfs_path.join("charge_full"))
                .context("unable to read charge_full sysfs file")
                .and_then(|x| {
                    x.trim()
                        .parse::<usize>()
                        .context("unable to parse charge_full sysfs file")
                });

            let charge_full_design =
                std::fs::read_to_string(self.sysfs_path.join("charge_full_design"))
                    .context("unable to read charge_full_design sysfs file")
                    .and_then(|x| {
                        x.trim()
                            .parse::<usize>()
                            .context("unable to parse charge_full_design sysfs file")
                    });

            if let (Ok(charge_full), Ok(charge_full_design)) = (charge_full, charge_full_design) {
                Ok(charge_full as f64 / charge_full_design as f64)
            } else {
                bail!("no health information found")
            }
        }
    }

    pub fn power_usage(&self) -> Result<f64> {
        std::fs::read_to_string(self.sysfs_path.join("power_now"))?
            .trim()
            .parse::<usize>()
            .map(|microwatts| microwatts as f64 / 1_000_000.0)
            .context("unable to parse power_now sysfs file")
    }

    pub fn state(&self) -> Result<State> {
        State::from_str(
            &std::fs::read_to_string(self.sysfs_path.join("status"))
                .map(|s| s.replace('\n', ""))?,
        )
    }

    pub fn charge_cycles(&self) -> Result<usize> {
        std::fs::read_to_string(self.sysfs_path.join("cycle_count"))?
            .trim()
            .parse()
            .context("unable to parse power_now sysfs file")
    }
}

impl Sensor for Battery {
    fn get_type_name(&self) -> &'static str {
        "Battery"
    }

    fn get_id(&self) -> String {
        self.sysfs_path.to_str().or_unk_owned()
    }

    fn get_name(&self) -> String {
        self.sysfs_path
            .file_name()
            .or_unk(|e| e.to_str().or_nan_owned())
    }
}
