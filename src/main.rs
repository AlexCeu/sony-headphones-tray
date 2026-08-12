// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright (C) 2026 TheOneProgrammer
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 2 of the License, or
// (at your option) any later version.

use ksni::menu::{MenuItem, RadioGroup, RadioItem, StandardItem, SubMenu};
use ksni::{blocking::TrayMethods, ToolTip, Tray};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const SERVICE_UUID: &str = "96cc203e-5068-46ad-b32d-e316f5e069ba";
const AF_BLUETOOTH: libc::c_int = 31;
const BTPROTO_RFCOMM: libc::c_int = 3;

static EXITING: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, PartialEq, Debug)]
enum Profile {
    NoiseCancelling,
    WindNoiseCancelling,
    AmbientSound,
    NoAnc,
}

impl Profile {
    const ALL: [Profile; 4] = [
        Profile::NoiseCancelling,
        Profile::WindNoiseCancelling,
        Profile::AmbientSound,
        Profile::NoAnc,
    ];

    fn key(self) -> &'static str {
        match self {
            Profile::NoiseCancelling => "noise-cancelling",
            Profile::WindNoiseCancelling => "wind-cancelling",
            Profile::AmbientSound => "ambient-sound",
            Profile::NoAnc => "disable",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Profile::NoiseCancelling => "Cancelación de ruido",
            Profile::WindNoiseCancelling => "Reducción de viento",
            Profile::AmbientSound => "Sonido ambiente",
            Profile::NoAnc => "Sin cancelación (OFF)",
        }
    }

    fn from_key(s: &str) -> Option<Profile> {
        Self::ALL.iter().copied().find(|p| p.key() == s)
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|&p| p == self).unwrap()
    }

    fn from_index(i: usize) -> Profile {
        Self::ALL[i.min(Self::ALL.len() - 1)]
    }

    fn ambient_bytes(self) -> [u8; 12] {
        let (enabled, noise_cancelling, volume, voice): (bool, u8, u8, bool) = match self {
            Profile::NoiseCancelling => (true, 2, 0, false),
            Profile::WindNoiseCancelling => (true, 1, 0, false),
            Profile::AmbientSound => (true, 0, 19, false),
            Profile::NoAnc => (false, 0, 0, false),
        };
        let enabled_value = if enabled { 16 } else { 0 };
        [
            0, 0, 0, 8, 104, 2, enabled_value, 2, noise_cancelling, 1, voice as u8, volume,
        ]
    }
}

fn build_packet(profile: Profile) -> Vec<u8> {
    let mut packet = Vec::with_capacity(14);
    packet.push(12);
    packet.push(0);
    packet.extend_from_slice(&profile.ambient_bytes());

    let crc: u8 = packet.iter().fold(0u8, |acc, b| acc.wrapping_add(*b));

    let mut full = Vec::with_capacity(packet.len() + 3);
    full.push(62);
    full.extend_from_slice(&packet);
    full.push(crc);
    full.push(60);
    full
}

#[repr(C)]
struct SockaddrRc {
    rc_family: u16,
    rc_bdaddr: [u8; 6],
    rc_channel: u8,
}

fn str2ba(addr: &str) -> Result<[u8; 6], String> {
    let parts: Vec<&str> = addr.trim().split(':').collect();
    if parts.len() != 6 {
        return Err(format!("MAC inválida: {addr}"));
    }
    let mut bytes = [0u8; 6];
    for (i, hex) in parts.iter().enumerate() {
        bytes[i] = u8::from_str_radix(hex, 16).map_err(|e| format!("MAC inválida: {e}"))?;
    }
    let mut out = [0u8; 6];
    for i in 0..6 {
        out[i] = bytes[5 - i];
    }
    Ok(out)
}

fn find_service_channel(mac: &str) -> u8 {
    if let Ok(output) = Command::new("sdptool")
        .args(["search", "--uuid", SERVICE_UUID, mac])
        .output()
    {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            if let Some(rest) = line.trim().strip_prefix("Channel:") {
                if let Ok(n) = rest.trim().parse::<u8>() {
                    return n;
                }
            }
        }
    }
    9
}

fn send_mode(mac: &str, profile: Profile) -> Result<(), String> {
    let channel = find_service_channel(mac);
    let bdaddr = str2ba(mac)?;

    let fd = unsafe { libc::socket(AF_BLUETOOTH, libc::SOCK_STREAM, BTPROTO_RFCOMM) };
    if fd < 0 {
        return Err(format!("No se pudo crear socket: {}, ¿probarás con Bluetooth activo?", std::io::Error::last_os_error()));
    }

    let sockaddr = SockaddrRc {
        rc_family: AF_BLUETOOTH as u16,
        rc_bdaddr: bdaddr,
        rc_channel: channel,
    };
    let ret = unsafe {
        libc::connect(
            fd,
            &sockaddr as *const SockaddrRc as *const libc::sockaddr,
            std::mem::size_of::<SockaddrRc>() as libc::socklen_t,
        )
    };
    if ret != 0 {
        unsafe { libc::close(fd) };
        return Err(format!(
            "No se pudo conectar ({mac}:{channel}): {}",
            std::io::Error::last_os_error()
        ));
    }

    let packet = build_packet(profile);
    let sent = unsafe {
        libc::send(
            fd,
            packet.as_ptr() as *const libc::c_void,
            packet.len(),
            libc::MSG_NOSIGNAL,
        )
    };
    unsafe { libc::close(fd) };
    if sent < 0 {
        return Err(format!("Fallo al enviar: {}", std::io::Error::last_os_error()));
    }
    Ok(())
}

fn notify(title: &str, body: &str) {
    let title = title.to_string();
    let body = body.to_string();
    std::thread::spawn(move || {
        let ok = Command::new("notify-send")
            .arg(&title)
            .arg(&body)
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false);
        if !ok {
            let _ = Command::new("kdialog")
                .args(["--title", &title, "--passivepopup", &body, "3500"])
                .output();
        }
    });
}

fn show_about() {
    let text = format!(
        "Sony XM4 Tray v{}\n\
         \n\
         Control del modo de sonido de auriculares Sony\ndesde la bandeja de KDE.\n\
         \n\
         Compatibles (mismo protocolo RFCOMM):\n\
         WH-1000XM2/XM3/XM4 · WF-1000XM3 · WH-XB900N · WI-1000X\n\
         \n\
         Autor: TheOneProgrammer\n\
         Web: https://theoneprogrammer.com\n\
         \n\
         Código desarrollado y revisado con asistencia de IA (opencode).\n\
         Protocolo original: ClusterM / sony-headphones-control.",
        env!("CARGO_PKG_VERSION")
    );
    std::thread::spawn(move || {
        let _ = Command::new("kdialog")
            .args(["--title", "Acerca de Sony XM4 Tray", "--msgbox", &text])
            .output();
    });
}

fn open_url(url: &str) {
    let url = url.to_string();
    std::thread::spawn(move || {
        let _ = Command::new("xdg-open").arg(&url).output();
    });
}

fn device_label(mac: &str) -> String {
    list_sony_devices()
        .into_iter()
        .find(|(m, _)| m == mac)
        .map(|(_, n)| n)
        .unwrap_or_else(|| mac.to_string())
}

#[derive(Clone)]
struct State {
    mac: String,
    profile: Profile,
    msg: String,
    devices: Vec<(String, String)>,
    devices_fetched_at: Option<Instant>,
}

fn parse_sony_devices(output: &str) -> Vec<(String, String)> {
    const MODELS: [&str; 6] = ["xm4", "wh-1000", "wh-xb900", "wi-1000", "wf-1000", "sony"];
    let mut found = Vec::new();
    for line in output.lines() {
        let mut parts = line.split_whitespace();
        if parts.next() != Some("Device") {
            continue;
        }
        let Some(mac) = parts.next() else { continue };
        let name = parts.collect::<Vec<_>>().join(" ");
        let lower = name.to_lowercase();
        if MODELS.iter().any(|m| lower.contains(m)) {
            found.push((mac.to_string(), name));
        }
    }
    found
}

fn list_sony_devices() -> Vec<(String, String)> {
    let Ok(output) = Command::new("bluetoothctl").arg("devices").output() else {
        return Vec::new();
    };
    parse_sony_devices(&String::from_utf8_lossy(&output.stdout))
}

#[derive(Clone)]
struct SonyTray {
    state: Arc<Mutex<State>>,
}

impl SonyTray {
    fn refresh_devices(&self) {
        let mut st = self.state.lock().unwrap();
        st.devices = list_sony_devices();
        st.devices_fetched_at = Some(Instant::now());
        if st.devices.is_empty() {
            st.msg = "No se encontró ningún XM4 pareado".into();
        }
    }

    fn set_profile(&self, profile: Profile) {
        let mac = {
            let mut st = self.state.lock().unwrap();
            st.profile = profile;
            save_profile(profile);
            if !st.mac.trim().is_empty() {
                st.msg = format!("Enviando {}…", profile.label());
                st.mac.clone()
            } else {
                st.msg = "Configura la MAC del dispositivo".into();
                String::new()
            }
        };
        if mac.is_empty() {
            return;
        }
        let state = Arc::clone(&self.state);
        let mac = mac.clone();
        std::thread::spawn(move || {
            let result = send_mode(&mac, profile);
            let status = match result {
                Ok(()) => format!("{} ✓", profile.label()),
                Err(e) => format!("Error: {e}"),
            };
            state.lock().unwrap().msg = status.clone();
            notify("Sony XM4", &status);
            if let Some(handle) = HANDLE.get() {
                handle.update(|_| {});
            }
        });
    }
}

static HANDLE: std::sync::OnceLock<ksni::blocking::Handle<SonyTray>> = std::sync::OnceLock::new();

impl Tray for SonyTray {
    const MENU_ON_ACTIVATE: bool = true;

    fn id(&self) -> String {
        "sony-xm4-tray".into()
    }

    fn title(&self) -> String {
        "Sony XM4".into()
    }

    fn icon_name(&self) -> String {
        "audio-headphones-bluetooth".into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::Hardware
    }

    fn tool_tip(&self) -> ToolTip {
        let st = self.state.lock().unwrap();
        ToolTip {
            title: "Sony WH-1000XM4".into(),
            description: format!("Perfil: {}\n{}", st.profile.label(), st.msg),
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let st = self.state.lock().unwrap();

        let device_submenu = {
            let mut children: Vec<MenuItem<Self>> = Vec::new();
            if st.devices.is_empty() {
                children.push(
                    StandardItem {
                        label: "No se encontró ningún XM4 pareado".into(),
                        enabled: false,
                        disposition: ksni::menu::Disposition::Informative,
                        ..Default::default()
                    }
                    .into(),
                );
            } else {
                let selected = st
                    .devices
                    .iter()
                    .position(|(mac, _)| *mac == st.mac)
                    .unwrap_or(usize::MAX);
                children.push(
                    RadioGroup {
                        selected,
                        select: Box::new(|tray: &mut Self, index: usize| {
                            if let Some((mac, _)) = tray.state.lock().unwrap().devices.get(index) {
                                let mac = mac.clone();
                                let mut st = tray.state.lock().unwrap();
                                st.mac = mac.clone();
                                st.msg = format!("MAC guardada: {mac}");
                                save_mac(&mac);
                            }
                        }),
                        options: st
                            .devices
                            .iter()
                            .map(|(mac, name)| RadioItem {
                                label: if name.trim().is_empty() {
                                    mac.as_str().into()
                                } else {
                                    format!("{name} ({mac})").into()
                                },
                                ..Default::default()
                            })
                            .collect(),
                    }
                    .into(),
                );
            }
            children.push(MenuItem::Separator);
            children.push(
                StandardItem {
                    label: "Actualizar lista (bluetoothctl)".into(),
                    icon_name: "view-refresh".into(),
                    activate: Box::new(|tray: &mut Self| tray.refresh_devices()),
                    ..Default::default()
                }
                .into(),
            );
            children
        };

        vec![
            StandardItem {
                label: format!("Perfil actual: {}", st.profile.label()),
                disposition: ksni::menu::Disposition::Informative,
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            RadioGroup {
                selected: st.profile.index(),
                select: Box::new(move |tray: &mut Self, index: usize| {
                    let profile = Profile::from_index(index);
                    tray.set_profile(profile);
                }),
                options: Profile::ALL
                    .iter()
                    .map(|p| RadioItem {
                        label: p.label().into(),
                        ..Default::default()
                    })
                    .collect(),
            }
            .into(),
            SubMenu {
                label: "Elegir dispositivo (Bluetooth)".into(),
                icon_name: "bluetooth-active".into(),
                submenu: device_submenu,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: st.msg.clone(),
                disposition: ksni::menu::Disposition::Informative,
                enabled: false,
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Reenviar perfil actual".into(),
                activate: Box::new(|tray: &mut Self| {
                    let profile = tray.state.lock().unwrap().profile;
                    tray.set_profile(profile);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Ver MAC / configuración".into(),
                activate: Box::new(|tray: &mut Self| {
                    let mut st = tray.state.lock().unwrap();
                    st.msg = if st.mac.trim().is_empty() {
                        let t = format!(
                            "MAC no configurada: ponla en {} o elige el dispositivo en el menú",
                            config_dir().join("mac").display()
                        );
                        notify("Sony XM4", &t);
                        t
                    } else {
                        let t = format!("Dispositivo actual: {}\nMAC: {}", device_label(&st.mac), st.mac);
                        notify("Sony XM4", &t);
                        t
                    };
                }),
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: format!("Acerca de (v{})", env!("CARGO_PKG_VERSION")),
                icon_name: "help-about".into(),
                submenu: vec![
                    StandardItem {
                        label: "Sobre la app".into(),
                        icon_name: "help-about".into(),
                        activate: Box::new(|_| show_about()),
                        ..Default::default()
                    }
                    .into(),
                    MenuItem::Separator,
                    StandardItem {
                        label: "Autor: TheOneProgrammer".into(),
                        disposition: ksni::menu::Disposition::Informative,
                        enabled: false,
                        ..Default::default()
                    }
                    .into(),
                    StandardItem {
                        label: "Visitar theoneprogrammer.com".into(),
                        icon_name: "internet-web-browser".into(),
                        activate: Box::new(|_| open_url("https://theoneprogrammer.com")),
                        ..Default::default()
                    }
                    .into(),
                    StandardItem {
                        label: "Proyecto base: sony-headphones-control".into(),
                        icon_name: "internet-web-browser".into(),
                        activate: Box::new(|_| {
                            open_url("https://github.com/ClusterM/sony-headphones-control")
                        }),
                        ..Default::default()
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Salir".into(),
                activate: Box::new(|_| {
                    EXITING.store(true, Ordering::Relaxed);
                    if let Some(h) = HANDLE.get() {
                        let _ = h.shutdown();
                    }
                }),
                ..Default::default()
            }
            .into(),
        ]
    }

    fn menu_about_to_show(&mut self) {
        let mut st = self.state.lock().unwrap();
        let stale = st
            .devices_fetched_at
            .map(|t| t.elapsed().as_secs() > 5)
            .unwrap_or(true);
        if stale {
            st.devices = list_sony_devices();
            st.devices_fetched_at = Some(Instant::now());
        }
    }
}

fn config_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg).join("sony-xm4-tray");
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".config").join("sony-xm4-tray")
}

fn load_mac() -> String {
    if let Ok(m) = std::env::var("XM4_MAC") {
        if !m.trim().is_empty() {
            return m.trim().into();
        }
    }
    let p = config_dir().join("mac");
    std::fs::read_to_string(&p)
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn save_profile(profile: Profile) {
    let dir = config_dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("profile"), format!("{}\n", profile.key()));
}

fn save_mac(mac: &str) {
    let dir = config_dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("mac"), format!("{mac}\n"));
}

fn load_profile() -> Profile {
    let p = config_dir().join("profile");
    if let Ok(s) = std::fs::read_to_string(&p) {
        if let Some(pf) = Profile::from_key(s.trim()) {
            return pf;
        }
    }
    Profile::NoiseCancelling
}

fn main() {
    let mut mac = load_mac();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(pos) = args.iter().position(|a| a == "--mac") {
        if let Some(v) = args.get(pos + 1) {
            mac = v.clone();
        }
    }

    if mac.trim().is_empty() {
        let found = list_sony_devices();
        match found.len() {
            1 => {
                mac = found[0].0.clone();
                save_mac(&mac);
                println!("Dispositivo detectado automáticamente: {} ({mac})", found[0].1);
            }
            0 => eprintln!(
                "Aviso: no hay MAC configurada ni XM4 pareado detectado.\n  Empareja los auriculares o pon la MAC en {} /mac\n  También puedes elegirla desde el menú de la bandeja",
                config_dir().display()
            ),
            _ => eprintln!(
                "Se encontraron varios XM4: elige el dispositivo desde el menú de la bandeja."
            ),
        }
    }

    let devices = list_sony_devices();
    let state = Arc::new(Mutex::new(State {
        mac: mac.clone(),
        profile: load_profile(),
        msg: if mac.trim().is_empty() {
            "Sin MAC: elige dispositivo en el menú".into()
        } else {
            format!("Listo ({mac})")
        },
        devices,
        devices_fetched_at: Some(Instant::now()),
    }));

    let handle = SonyTray { state: Arc::clone(&state) }
        .spawn()
        .unwrap_or_else(|e| {
            eprintln!("No se pudo arrancar la bandeja: {e}");
            std::process::exit(1);
        });

    let _ = HANDLE.set(handle.clone());

    while !EXITING.load(Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    handle.shutdown().wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_matches_python() {
        // noise-cancelling: getAmbientSound(True, 2, 0, False)
        let packet = build_packet(Profile::NoiseCancelling);
        assert_eq!(
            packet,
            vec![62, 12, 0, 0, 0, 0, 8, 104, 2, 16, 2, 2, 1, 0, 0, 147, 60]
        );
        // disable: getAmbientSound(False, 0, 0, False)
        let packet = build_packet(Profile::NoAnc);
        assert_eq!(packet, vec![62, 12, 0, 0, 0, 0, 8, 104, 2, 0, 2, 0, 1, 0, 0, 129, 60]);
    }

    #[test]
    fn str2ba_reverses_bytes() {
        assert_eq!(
            str2ba("aa:bb:cc:dd:ee:ff").unwrap(),
            [0xff, 0xee, 0xdd, 0xcc, 0xbb, 0xaa]
        );
        assert!(str2ba("no-es-mac").is_err());
    }

    #[test]
    fn parses_sony_devices_from_bluetoothctl() {
        let output = "Device C4:EF:DA:1F:33:58 CK67\n\
                      Device 88:C9:E8:60:6F:D9 WH-1000XM4\n\
                      Device 90:7F:61:39:22:42 Lenovo Active Pen2\n\
                      Device A0:1B:2C:3D:4F:60 WH-1000XM2\n\
                      Device B0:1B:2C:3D:4F:70 WH-XB900N\n\
                      Device C0:1B:2C:3D:4F:80 WI-1000X\n\
                      Device D0:1B:2C:3D:4F:90 WF-1000XM3\n";
        let found = parse_sony_devices(output);
        assert_eq!(
            found,
            vec![
                ("88:C9:E8:60:6F:D9".to_string(), "WH-1000XM4".to_string()),
                ("A0:1B:2C:3D:4F:60".to_string(), "WH-1000XM2".to_string()),
                ("B0:1B:2C:3D:4F:70".to_string(), "WH-XB900N".to_string()),
                ("C0:1B:2C:3D:4F:80".to_string(), "WI-1000X".to_string()),
                ("D0:1B:2C:3D:4F:90".to_string(), "WF-1000XM3".to_string()),
            ]
        );
        assert!(parse_sony_devices("").is_empty());
    }
}