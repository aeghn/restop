use std::process::Command;

use anyhow::{bail, Context, Result};
use once_cell::sync::Lazy;
use regex::Regex;

const TEMPLATE_RE_PRESENT: &str = r"MEMORY_DEVICE_%_PRESENT=(\d)";

const TEMPLATE_RE_CONFIGURED_SPEED_MTS: &str = r"MEMORY_DEVICE_%_CONFIGURED_SPEED_MTS=(\d*)";

const TEMPLATE_RE_SPEED_MTS: &str = r"MEMORY_DEVICE_%_SPEED_MTS=(\d*)";

const TEMPLATE_RE_FORM_FACTOR: &str = r"MEMORY_DEVICE_%_FORM_FACTOR=(.*)";

const TEMPLATE_RE_TYPE: &str = r"MEMORY_DEVICE_%_TYPE=(.*)";

const TEMPLATE_RE_TYPE_DETAIL: &str = r"MEMORY_DEVICE_%_TYPE_DETAIL=(.*)";

const TEMPLATE_RE_SIZE: &str = r"MEMORY_DEVICE_%_SIZE=(\d*)";

const BYTES_IN_GIB: u64 = 1_073_741_824; // 1024 * 1024 * 1024

static RE_CONFIGURED_SPEED: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"Configured Memory Speed: (\d+) MT/s").unwrap());

static RE_SPEED: Lazy<Regex> = Lazy::new(|| Regex::new(r"Speed: (\d+) MT/s").unwrap());

static RE_FORMFACTOR: Lazy<Regex> = Lazy::new(|| Regex::new(r"Form Factor: (.+)").unwrap());

static RE_TYPE: Lazy<Regex> = Lazy::new(|| Regex::new(r"Type: (.+)").unwrap());

static RE_TYPE_DETAIL: Lazy<Regex> = Lazy::new(|| Regex::new(r"Type Detail: (.+)").unwrap());

static RE_SIZE: Lazy<Regex> = Lazy::new(|| Regex::new(r"Size: (\d+) GB").unwrap());

static RE_MEM_TOTAL: Lazy<Regex> = Lazy::new(|| Regex::new(r"MemTotal:\s*(\d*) kB").unwrap());

static RE_MEM_AVAILABLE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"MemAvailable:\s*(\d*) kB").unwrap());

static RE_SWAP_TOTAL: Lazy<Regex> = Lazy::new(|| Regex::new(r"SwapTotal:\s*(\d*) kB").unwrap());

static RE_SWAP_FREE: Lazy<Regex> = Lazy::new(|| Regex::new(r"SwapFree:\s*(\d*) kB").unwrap());

static RE_NUM_MEMORY_DEVICES: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"MEMORY_ARRAY_NUM_DEVICES=(\d*)").unwrap());

#[derive(Debug, Clone, Copy)]
pub struct MemoryData {
    pub total_mem: usize,
    pub available_mem: usize,
    pub total_swap: usize,
    pub free_swap: usize,
}

impl MemoryData {
    pub fn fetch(_req: ()) -> Result<Self> {
        let proc_mem =
            std::fs::read_to_string("/proc/meminfo").context("unable to read /proc/meminfo")?;

        let total_mem = RE_MEM_TOTAL
            .captures(&proc_mem)
            .context("RE_MEM_TOTAL no captures")
            .and_then(|captures| {
                captures
                    .get(1)
                    .context("RE_MEM_TOTAL not enough captures")
                    .and_then(|capture| {
                        capture
                            .as_str()
                            .parse::<usize>()
                            .context("unable to parse MemTotal")
                            .map(|int| int * 1024)
                    })
            })?;

        let available_mem = RE_MEM_AVAILABLE
            .captures(&proc_mem)
            .context("RE_MEM_AVAILABLE no captures")
            .and_then(|captures| {
                captures
                    .get(1)
                    .context("RE_MEM_AVAILABLE not enough captures")
                    .and_then(|capture| {
                        capture
                            .as_str()
                            .parse::<usize>()
                            .context("unable to parse MemAvailable")
                            .map(|int| int * 1024)
                    })
            })?;

        let total_swap = RE_SWAP_TOTAL
            .captures(&proc_mem)
            .context("RE_SWAP_TOTAL no captures")
            .and_then(|captures| {
                captures
                    .get(1)
                    .context("RE_SWAP_TOTAL not enough captures")
                    .and_then(|capture| {
                        capture
                            .as_str()
                            .parse::<usize>()
                            .context("unable to parse SwapTotal")
                            .map(|int| int * 1024)
                    })
            })?;

        let free_swap = RE_SWAP_FREE
            .captures(&proc_mem)
            .context("RE_SWAP_FREE no captures")
            .and_then(|captures| {
                captures
                    .get(1)
                    .context("RE_SWAP_FREE not enough captures")
                    .and_then(|capture| {
                        capture
                            .as_str()
                            .parse::<usize>()
                            .context("unable to parse SwapFree")
                            .map(|int| int * 1024)
                    })
            })?;

        Ok(Self {
            total_mem,
            available_mem,
            total_swap,
            free_swap,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct MemoryDevice {
    pub speed_mts: Option<u32>,
    pub form_factor: Option<String>,
    pub r#type: Option<String>,
    pub type_detail: Option<String>,
    pub size: Option<u64>,
    pub installed: bool,
}

fn parse_dmidecode<S: AsRef<str>>(dmi: S) -> Vec<MemoryDevice> {
    let mut devices = Vec::new();

    let device_strings = dmi.as_ref().split("\n\n");

    for device_string in device_strings {
        if device_string.is_empty() {
            continue;
        }
        let memory_device = MemoryDevice {
            speed_mts: RE_CONFIGURED_SPEED
                .captures(device_string)
                .or_else(|| RE_SPEED.captures(device_string))
                .map(|x| x[1].parse().unwrap()),
            form_factor: RE_FORMFACTOR
                .captures(device_string)
                .map(|x| x[1].to_string()),
            r#type: RE_TYPE.captures(device_string).map(|x| x[1].to_string()),
            type_detail: RE_TYPE_DETAIL
                .captures(device_string)
                .map(|x| x[1].to_string()),
            size: RE_SIZE
                .captures(device_string)
                .map(|x| x[1].parse::<u64>().unwrap() * BYTES_IN_GIB),
            installed: RE_SPEED
                .captures(device_string)
                .map(|x| x[1].to_string())
                .is_some(),
        };

        devices.push(memory_device);
    }

    devices
}

fn virtual_dmi() -> Vec<MemoryDevice> {
    let command = Command::new("udevadm")
        .args(["info", "-p", "/sys/devices/virtual/dmi/id"])
        .output();

    let virtual_dmi_output = command
        .context("unable to execute udevadm")
        .and_then(|output| {
            String::from_utf8(output.stdout).context("unable to parse stdout of udevadm to UTF-8")
        })
        .unwrap_or_default();

    parse_virtual_dmi(virtual_dmi_output)
}

fn parse_virtual_dmi<S: AsRef<str>>(dmi: S) -> Vec<MemoryDevice> {
    let dmi = dmi.as_ref();

    let devices_amount: usize = RE_NUM_MEMORY_DEVICES
        .captures(dmi)
        .and_then(|captures| captures.get(1))
        .and_then(|capture| capture.as_str().parse().ok())
        .unwrap_or(0);

    let mut devices = Vec::with_capacity(devices_amount);

    for i in 0..devices_amount {
        let i = i.to_string();

        let speed = Regex::new(&TEMPLATE_RE_CONFIGURED_SPEED_MTS.replace('%', &i))
            .ok()
            .and_then(|regex| regex.captures(dmi))
            .or_else(|| {
                Regex::new(&TEMPLATE_RE_SPEED_MTS.replace('%', &i.to_string()))
                    .ok()
                    .and_then(|regex| regex.captures(dmi))
            })
            .and_then(|captures| captures.get(1))
            .and_then(|capture| capture.as_str().parse().ok());

        let form_factor = Regex::new(&TEMPLATE_RE_FORM_FACTOR.replace('%', &i))
            .ok()
            .and_then(|regex| regex.captures(dmi))
            .and_then(|captures| captures.get(1))
            .map(|capture| capture.as_str().to_string());

        let r#type = Regex::new(&TEMPLATE_RE_TYPE.replace('%', &i))
            .ok()
            .and_then(|regex| regex.captures(dmi))
            .and_then(|captures| captures.get(1))
            .map(|capture| capture.as_str().to_string())
            .filter(|capture| capture != "<OUT OF SPEC>");

        let type_detail = Regex::new(&TEMPLATE_RE_TYPE_DETAIL.replace('%', &i))
            .ok()
            .and_then(|regex| regex.captures(dmi))
            .and_then(|captures| captures.get(1))
            .map(|capture| capture.as_str().to_string());

        let size = Regex::new(&TEMPLATE_RE_SIZE.replace('%', &i))
            .ok()
            .and_then(|regex| regex.captures(dmi))
            .and_then(|captures| captures.get(1))
            .and_then(|capture| capture.as_str().parse().ok());

        let installed = Regex::new(&TEMPLATE_RE_PRESENT.replace('%', &i))
            .ok()
            .and_then(|regex| regex.captures(dmi))
            .and_then(|captures| captures.get(1))
            .and_then(|capture| capture.as_str().parse::<usize>().ok())
            .map_or(true, |int| int != 0);

        devices.push(MemoryDevice {
            speed_mts: speed,
            form_factor,
            r#type,
            type_detail,
            size,
            installed,
        });
    }

    devices
}

pub fn get_memory_devices() -> Result<Vec<MemoryDevice>> {
    let virtual_dmi = virtual_dmi();
    if virtual_dmi.is_empty() {
        let output = Command::new("dmidecode")
            .args(["-t", "17", "-q"])
            .output()?;
        if output.status.code().unwrap_or(1) == 1 {
            tracing::debug!("Unable to get memory information without elevated privileges");
            bail!("no permission")
        }
        tracing::debug!("Memory information obtained using dmidecode (unprivileged)");
        Ok(parse_dmidecode(String::from_utf8(output.stdout)?))
    } else {
        tracing::debug!("Memory information obtained using udevadm");
        Ok(virtual_dmi)
    }
}

pub fn pkexec_dmidecode() -> Result<Vec<MemoryDevice>> {
    tracing::debug!("Using pkexec to get memory information (dmidecode)…");

    let output = Command::new("pkexec")
        .args(["--disable-internal-agent", "dmidecode", "-t", "17", "-q"])
        .output()?;

    tracing::debug!("Memory information obtained using dmidecode (privileged)");
    Ok(parse_dmidecode(String::from_utf8(output.stdout)?.as_str()))
}
