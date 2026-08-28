use serde::Serialize;
use std::collections::HashSet;
use std::process::Command;

pub const PASSWORD_ACCOUNT: &str = "easyinput.keyboard.wifi-password.v1";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WifiNetwork {
    pub ssid: String,
    pub current: bool,
    pub remembered: bool,
    pub configured: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WifiScanResult {
    pub interface: String,
    pub current_ssid: Option<String>,
    pub local_ip: Option<String>,
    pub networks: Vec<WifiNetwork>,
    pub warning: Option<String>,
}

fn command_stdout(path: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(path)
        .args(args)
        .output()
        .map_err(|error| format!("无法运行 {path}：{error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if !output.status.success() || stdout.contains("AuthorizationCreate() failed") {
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        return Err(if detail.is_empty() {
            format!("{path} 返回状态 {}", output.status)
        } else {
            detail
        });
    }
    Ok(stdout)
}

fn parse_wifi_interface(output: &str) -> Option<String> {
    let mut wifi_block = false;
    for line in output.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed.strip_prefix("Hardware Port:") {
            let name = name.trim();
            wifi_block = name.eq_ignore_ascii_case("Wi-Fi")
                || name.eq_ignore_ascii_case("AirPort");
            continue;
        }
        if wifi_block {
            if let Some(device) = trimmed.strip_prefix("Device:") {
                let device = device.trim();
                if !device.is_empty() {
                    return Some(device.to_owned());
                }
            }
        }
    }
    None
}

fn parse_preferred_networks(output: &str) -> Vec<String> {
    let mut networks = Vec::new();
    let mut in_list = false;
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.to_ascii_lowercase().starts_with("preferred networks on ") {
            in_list = true;
            continue;
        }
        if in_list && !trimmed.is_empty() {
            networks.push(trimmed.to_owned());
        }
    }
    networks
}

fn parse_current_network(output: &str) -> Option<String> {
    if output.to_ascii_lowercase().contains("not associated") {
        return None;
    }
    output.lines().find_map(|line| {
        let trimmed = line.trim();
        let (_, value) = trimmed.split_once(':')?;
        let value = value.trim();
        (!value.is_empty()).then(|| value.to_owned())
    })
}

fn append_network(
    networks: &mut Vec<WifiNetwork>,
    seen: &mut HashSet<String>,
    ssid: &str,
    current: bool,
    remembered: bool,
    configured: bool,
) {
    let ssid = ssid.trim();
    if ssid.is_empty() {
        return;
    }
    if let Some(existing) = networks.iter_mut().find(|item| item.ssid == ssid) {
        existing.current |= current;
        existing.remembered |= remembered;
        existing.configured |= configured;
        return;
    }
    if seen.insert(ssid.to_owned()) {
        networks.push(WifiNetwork {
            ssid: ssid.to_owned(),
            current,
            remembered,
            configured,
        });
    }
}

#[cfg(target_os = "macos")]
pub fn scan(configured_ssid: &str) -> Result<WifiScanResult, String> {
    let hardware = command_stdout("/usr/sbin/networksetup", &["-listallhardwareports"])?;
    let interface = parse_wifi_interface(&hardware)
        .ok_or_else(|| "macOS 没有返回可用的 Wi-Fi 接口".to_string())?;
    let mut warnings = Vec::new();
    let preferred = match command_stdout(
        "/usr/sbin/networksetup",
        &["-listpreferredwirelessnetworks", &interface],
    ) {
        Ok(output) => parse_preferred_networks(&output),
        Err(error) => {
            warnings.push(format!("无法读取系统已记住的网络：{error}"));
            Vec::new()
        }
    };
    let current_ssid = match command_stdout(
        "/usr/sbin/networksetup",
        &["-getairportnetwork", &interface],
    ) {
        Ok(output) => parse_current_network(&output),
        Err(error) => {
            warnings.push(format!("无法读取当前网络：{error}"));
            None
        }
    };
    let local_ip = command_stdout("/usr/sbin/ipconfig", &["getifaddr", &interface])
        .ok()
        .filter(|value| !value.is_empty());

    let mut networks = Vec::new();
    let mut seen = HashSet::new();
    if let Some(ssid) = current_ssid.as_deref() {
        append_network(
            &mut networks,
            &mut seen,
            ssid,
            true,
            preferred.iter().any(|item| item == ssid),
            ssid == configured_ssid,
        );
    }
    if !configured_ssid.trim().is_empty() {
        append_network(
            &mut networks,
            &mut seen,
            configured_ssid,
            current_ssid.as_deref() == Some(configured_ssid),
            preferred.iter().any(|item| item == configured_ssid),
            true,
        );
    }
    for ssid in preferred {
        append_network(
            &mut networks,
            &mut seen,
            &ssid,
            current_ssid.as_deref() == Some(ssid.as_str()),
            true,
            ssid == configured_ssid,
        );
    }

    Ok(WifiScanResult {
        interface,
        current_ssid,
        local_ip,
        networks,
        warning: (!warnings.is_empty()).then(|| warnings.join("；")),
    })
}

#[cfg(not(target_os = "macos"))]
pub fn scan(_configured_ssid: &str) -> Result<WifiScanResult, String> {
    Err("自动读取 Wi-Fi 当前仅支持 macOS".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_wifi_device_from_hardware_ports() {
        let output = "Hardware Port: Thunderbolt Bridge\nDevice: bridge0\n\nHardware Port: Wi-Fi\nDevice: en0\n";
        assert_eq!(parse_wifi_interface(output).as_deref(), Some("en0"));
    }

    #[test]
    fn parses_preferred_and_current_networks() {
        let preferred = "Preferred networks on en0:\n\tOffice 2.4G\n\tPhone Hotspot\n";
        assert_eq!(
            parse_preferred_networks(preferred),
            vec!["Office 2.4G", "Phone Hotspot"]
        );
        assert_eq!(
            parse_current_network("Current Wi-Fi Network: Office 2.4G").as_deref(),
            Some("Office 2.4G")
        );
        assert_eq!(
            parse_current_network("You are not associated with an AirPort network."),
            None
        );
    }

    #[test]
    fn merges_duplicate_network_flags() {
        let mut networks = Vec::new();
        let mut seen = HashSet::new();
        append_network(&mut networks, &mut seen, "Office", false, true, false);
        append_network(&mut networks, &mut seen, "Office", true, false, true);
        assert_eq!(networks.len(), 1);
        assert_eq!(
            networks[0],
            WifiNetwork {
                ssid: "Office".into(),
                current: true,
                remembered: true,
                configured: true,
            }
        );
    }
}
