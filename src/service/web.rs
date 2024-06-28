use super::data::*;
use anyhow::{bail, Context, Result};
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::fs::File;
use std::process::Command;
use std::sync::Arc;
use sysinfo::System;
#[derive(Debug, Default)]
pub struct ClashStatus {
    pub info: Option<StartBody>,
}
#[allow(dead_code)]
#[derive(Debug, Default)]
pub struct DNSStatus {
    pub dns: Option<String>,
}

impl ClashStatus {
    pub fn global() -> &'static Arc<Mutex<ClashStatus>> {
        static CLASHSTATUS: OnceCell<Arc<Mutex<ClashStatus>>> = OnceCell::new();

        CLASHSTATUS.get_or_init(|| Arc::new(Mutex::new(ClashStatus::default())))
    }
}

#[allow(dead_code)]
impl DNSStatus {
    pub fn global() -> &'static Arc<Mutex<DNSStatus>> {
        static DNSSTAUS: OnceCell<Arc<Mutex<DNSStatus>>> = OnceCell::new();

        DNSSTAUS.get_or_init(|| Arc::new(Mutex::new(DNSStatus::default())))
    }
}

/// GET /version
/// 获取服务进程的版本
pub fn get_version() -> Result<HashMap<String, String>> {
    let version = env!("CARGO_PKG_VERSION");

    let mut map = HashMap::new();

    map.insert("service".into(), "Clash Verge Service".into());
    map.insert("version".into(), version.into());

    Ok(map)
}

/// POST /start_clash
/// 启动clash进程
pub fn start_clash(body: StartBody) -> Result<()> {
    // stop the old clash bin
    let _ = stop_clash();

    let body_cloned = body.clone();

    let config_dir = body.config_dir.as_str();

    let config_file = body.config_file.as_str();

    let args = vec!["-d", config_dir, "-f", config_file];

    let log = File::create(body.log_file).context("failed to open log")?;
    Command::new(body.bin_path).args(args).stdout(log).spawn()?;

    let mut arc = ClashStatus::global().lock();
    arc.info = Some(body_cloned);

    Ok(())
}

/// POST /stop_clash
/// 停止clash进程
pub fn stop_clash() -> Result<()> {
    let mut arc = ClashStatus::global().lock();

    arc.info = None;

    let mut system = System::new();
    system.refresh_all();
    let procs = system.processes_by_name("verge-mihomo");
    for proc in procs {
        proc.kill();
    }
    Ok(())
}

/// GET /get_clash
/// 获取clash当前执行信息
pub fn get_clash() -> Result<StartBody> {
    let arc = ClashStatus::global().lock();

    match arc.info.clone() {
        Some(info) => Ok(info),
        None => bail!("clash not executed"),
    }
}

/// POST /set_dns
/// 设置DNS
pub fn set_dns() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let service = default_network_service().or_else(|e| default_network_service_by_ns());
        if let Err(e) = service {
            return Err(e);
        }
        let service = service.unwrap();
        let output = networksetup()
            .arg("-getdnsservers")
            .arg(&service)
            .output()?;
        let mut origin_dns = String::from_utf8(output.stdout)?;
        if origin_dns.trim().len() > 15 {
            origin_dns = "Empty".to_string();
        }
        let mut arc = DNSStatus::global().lock();
        arc.dns = Some(origin_dns);

        networksetup()
            .arg("-setdnsservers")
            .arg(&service)
            .arg("223.5.5.5")
            .output()?;
    }

    Ok(())
}

/// POST /unset_dns
/// 还原DNS
pub fn unset_dns() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let arc = DNSStatus::global().lock();

        let origin_dns = match arc.dns.clone() {
            Some(dns) => dns,
            None => "".to_string(),
        };
        if !origin_dns.is_empty() {
            let service = default_network_service().or_else(|e| default_network_service_by_ns());
            if let Err(e) = service {
                return Err(e);
            }
            let service = service.unwrap();
            networksetup()
                .arg("-setdnsservers")
                .arg(service)
                .arg(origin_dns)
                .output()?;
        }
    }

    Ok(())
}
#[cfg(target_os = "macos")]
fn networksetup() -> Command {
    Command::new("networksetup")
}

#[cfg(target_os = "macos")]
fn default_network_service() -> Result<String> {
    use std::net::{SocketAddr, UdpSocket};
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.connect("1.1.1.1:80")?;
    let ip = socket.local_addr()?.ip();
    let addr = SocketAddr::new(ip, 0);

    let interfaces = interfaces::Interface::get_all()?;
    let interface = interfaces
        .into_iter()
        .find(|i| i.addresses.iter().find(|a| a.addr == Some(addr)).is_some())
        .map(|i| i.name.to_owned());

    match interface {
        Some(interface) => {
            let service = get_server_by_order(interface)?;
            Ok(service)
        }
        None => anyhow::bail!("No network service found"),
    }
}

#[cfg(target_os = "macos")]
fn default_network_service_by_ns() -> Result<String> {
    let output = networksetup().arg("-listallnetworkservices").output()?;
    let stdout = String::from_utf8(output.stdout)?;
    let mut lines = stdout.split('\n');
    lines.next(); // ignore the tips

    // get the first service
    match lines.next() {
        Some(line) => Ok(line.into()),
        None => anyhow::bail!("No network service found"),
    }
}

#[cfg(target_os = "macos")]
fn get_server_by_order(device: String) -> Result<String> {
    let services = listnetworkserviceorder()?;
    let service = services
        .into_iter()
        .find(|(_, _, d)| d == &device)
        .map(|(s, _, _)| s);
    match service {
        Some(service) => Ok(service),
        None => anyhow::bail!("No network service found"),
    }
}

#[cfg(target_os = "macos")]
fn listnetworkserviceorder() -> Result<Vec<(String, String, String)>> {
    let output = networksetup().arg("-listnetworkserviceorder").output()?;
    let stdout = String::from_utf8(output.stdout)?;

    let mut lines = stdout.split('\n');
    lines.next(); // ignore the tips

    let mut services = Vec::new();
    let mut p: Option<(String, String, String)> = None;

    for line in lines {
        if !line.starts_with('(') {
            continue;
        }

        if p.is_none() {
            let ri = line.find(')');
            if ri.is_none() {
                continue;
            }
            let ri = ri.unwrap();
            let service = line[ri + 1..].trim();
            p = Some((service.into(), "".into(), "".into()));
        } else {
            let line = &line[1..line.len() - 1];
            let pi = line.find("Port:");
            let di = line.find(", Device:");
            if pi.is_none() || di.is_none() {
                continue;
            }
            let pi = pi.unwrap();
            let di = di.unwrap();
            let port = line[pi + 5..di].trim();
            let device = line[di + 9..].trim();
            let (service, _, _) = p.as_mut().unwrap();
            *p.as_mut().unwrap() = (service.to_owned(), port.into(), device.into());
            services.push(p.take().unwrap());
        }
    }

    Ok(services)
}
