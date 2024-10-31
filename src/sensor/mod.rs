use once_cell::sync::Lazy;

pub mod apps;
pub mod battery;
pub mod cpu;
pub mod drive;
pub mod gpu;
pub mod memory;
pub mod network;
pub mod pci;
pub mod process;
#[allow(unused_variables)]
pub mod settings;
pub mod time;
pub mod units;

static TICK_RATE: Lazy<usize> =
    Lazy::new(|| sysconf::sysconf(sysconf::SysconfVariable::ScClkTck).unwrap_or(100) as usize);

pub static NUM_CPUS: Lazy<usize> = Lazy::new(num_cpus::get);

// Adapted from Mission Center: https://gitlab.com/mission-center-devs/mission-center/
pub static IS_FLATPAK: Lazy<bool> = Lazy::new(|| {
    let is_flatpak = std::path::Path::new("/.flatpak-info").exists();

    /*     if is_flatpak {
        debug!("Running as Flatpak");
    } else {
        debug!("Not running as Flatpak");
    } */

    is_flatpak
});

pub trait Sensor {
    fn get_type_name(&self) -> &'static str;
    fn get_id(&self) -> String;
    fn get_name(&self) -> String;
}
