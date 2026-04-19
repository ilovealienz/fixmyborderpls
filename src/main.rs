#![windows_subsystem = "windows"]

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

use serde::{Deserialize, Serialize};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem},
    TrayIconBuilder, TrayIconEvent,
};
use winapi::shared::windef::HWND;
use winapi::um::dwmapi::DwmSetWindowAttribute;
use winapi::um::winreg::*;
use winapi::um::winnt::*;
use winapi::um::winuser::*;

mod icon_data;

const DWMWA_WINDOW_CORNER_PREF: u32 = 33;
const DWMWA_BORDER_COLOR: u32 = 34;
const DWMWCP_DONOTROUND: u32 = 1;
const DWMWCP_ROUND: u32 = 2;

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Config {
    /// Border colour — hex (#rrggbb or #rrggbbaa) or rgba(r,g,b,a)
    #[serde(default = "default_color")]
    color: String,
    /// true = square corners, false = rounded
    #[serde(default = "yes")]
    square_corners: bool,
    /// Add fixmyborderpls to Windows startup
    #[serde(default)]
    run_on_startup: bool,
}

fn default_color() -> String { "#000000".to_string() }
fn yes() -> bool { true }

impl Default for Config {
    fn default() -> Self {
        Self { color: default_color(), square_corners: true, run_on_startup: false }
    }
}

impl Config {
    /// Parse color string into COLORREF (0x00BBGGRR)
    fn to_colorref(&self) -> u32 {
        let (r, g, b, _a) = parse_color(&self.color).unwrap_or((0, 0, 0, 255));
        r as u32 | ((g as u32) << 8) | ((b as u32) << 16)
    }
}

/// Parse #rgb, #rrggbb, #rrggbbaa, or rgba(r,g,b,a) into (r,g,b,a) bytes.
fn parse_color(s: &str) -> Option<(u8, u8, u8, u8)> {
    let s = s.trim();

    // hex formats
    if let Some(hex) = s.strip_prefix('#') {
        return match hex.len() {
            3 => {
                let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
                let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
                let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
                Some((r, g, b, 255))
            }
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                Some((r, g, b, 255))
            }
            8 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
                Some((r, g, b, a))
            }
            _ => None,
        };
    }

    // rgba(r, g, b, a) — a can be 0.0–1.0 or 0–255
    if let Some(inner) = s.strip_prefix("rgba(").and_then(|s| s.strip_suffix(')')) {
        let parts: Vec<&str> = inner.split(',').collect();
        if parts.len() == 4 {
            let r = parts[0].trim().parse::<u8>().ok()?;
            let g = parts[1].trim().parse::<u8>().ok()?;
            let b = parts[2].trim().parse::<u8>().ok()?;
            let a_str = parts[3].trim();
            let a = if a_str.contains('.') {
                (a_str.parse::<f32>().ok()? * 255.0).round() as u8
            } else {
                a_str.parse::<u8>().ok()?
            };
            return Some((r, g, b, a));
        }
    }

    // rgb(r, g, b)
    if let Some(inner) = s.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')')) {
        let parts: Vec<&str> = inner.split(',').collect();
        if parts.len() == 3 {
            let r = parts[0].trim().parse::<u8>().ok()?;
            let g = parts[1].trim().parse::<u8>().ok()?;
            let b = parts[2].trim().parse::<u8>().ok()?;
            return Some((r, g, b, 255));
        }
    }

    None
}

// ── Persistence ───────────────────────────────────────────────────────────────

fn config_dir() -> PathBuf {
    let mut p = dirs_next::data_dir().unwrap_or_else(|| PathBuf::from("."));
    p.push("fixmyborderpls");
    fs::create_dir_all(&p).ok();
    p
}

fn config_path() -> PathBuf { config_dir().join("config.json") }

fn load_config() -> Config {
    fs::read_to_string(config_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_config(cfg: &Config) {
    if let Ok(s) = serde_json::to_string_pretty(cfg) {
        fs::write(config_path(), s).ok();
    }
}

// ── Shell helpers ─────────────────────────────────────────────────────────────

fn wstr(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

fn open_config_folder() {
    let path = wstr(config_dir().to_str().unwrap());
    unsafe {
        winapi::um::shellapi::ShellExecuteW(
            std::ptr::null_mut(),
            wstr("open").as_ptr(),
            path.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
    }
}

// ── Startup registry ──────────────────────────────────────────────────────────

fn set_startup(enable: bool) {
    let subkey: Vec<u16> = OsStr::new("Software\\Microsoft\\Windows\\CurrentVersion\\Run\0")
        .encode_wide().collect();
    let name: Vec<u16> = OsStr::new("fixmyborderpls\0").encode_wide().collect();
    unsafe {
        let mut hkey = std::ptr::null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, KEY_SET_VALUE, &mut hkey) != 0 { return; }
        if enable {
            let exe = std::env::current_exe().unwrap();
            let path: Vec<u16> = OsStr::new(exe.to_str().unwrap())
                .encode_wide().chain(Some(0)).collect();
            RegSetValueExW(hkey, name.as_ptr(), 0, REG_SZ, path.as_ptr() as _, (path.len() * 2) as u32);
        } else {
            RegDeleteValueW(hkey, name.as_ptr());
        }
        RegCloseKey(hkey);
    }
}

// ── DWM styling ───────────────────────────────────────────────────────────────

fn style_window(hwnd: HWND, cfg: &Config) {
    unsafe {
        let corner = if cfg.square_corners { DWMWCP_DONOTROUND } else { DWMWCP_ROUND };
        DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREF, &corner as *const _ as _, 4);
        let color = cfg.to_colorref();
        DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR, &color as *const _ as _, 4);
    }
}

unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: isize) -> i32 {
    use winapi::um::processthreadsapi::OpenProcess;
    use winapi::um::psapi::GetModuleBaseNameW;
    use winapi::um::winnt::PROCESS_QUERY_LIMITED_INFORMATION;

    if IsWindowVisible(hwnd) == 0 || GetWindowTextLengthW(hwnd) == 0 { return 1; }
    if !GetWindow(hwnd, GW_OWNER).is_null() { return 1; }
    let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
    if style & WS_CAPTION == 0 { return 1; }

    let mut pid: u32 = 0;
    GetWindowThreadProcessId(hwnd, &mut pid);
    let proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
    if !proc.is_null() {
        let mut buf = [0u16; 260];
        let len = GetModuleBaseNameW(proc, std::ptr::null_mut(), buf.as_mut_ptr(), 260);
        winapi::um::handleapi::CloseHandle(proc);
        if len > 0 {
            let exe = String::from_utf16_lossy(&buf[..len as usize]).to_lowercase();
            if matches!(exe.as_str(),
                "shellexperiencehost.exe"|"startmenuexperiencehost.exe"|"searchhost.exe"|
                "searchui.exe"|"cortana.exe"|"textinputhost.exe"|"lockapp.exe"|
                "logonui.exe"|"winlogon.exe"|"dwm.exe"
            ) { return 1; }
            if exe == "explorer.exe" {
                let mut class = [0u16; 256];
                let clen = GetClassNameW(hwnd, class.as_mut_ptr(), 256);
                if clen == 0 { return 1; }
                let cs = String::from_utf16_lossy(&class[..clen as usize]);
                if !matches!(cs.as_str(), "CabinetWClass"|"ExploreWClass") { return 1; }
            }
        }
    }
    style_window(hwnd, &*(lparam as *const Config));
    1
}

fn style_all(cfg: &Config) {
    unsafe { winapi::um::winuser::EnumWindows(Some(enum_cb), cfg as *const Config as isize); }
}

// ── Tray icon ─────────────────────────────────────────────────────────────────

fn make_icon() -> tray_icon::Icon {
    tray_icon::Icon::from_rgba(
        icon_data::ICON_32_RGBA.to_vec(),
        icon_data::ICON_32_W,
        icon_data::ICON_32_H,
    ).unwrap()
}


// ── Main ──────────────────────────────────────────────────────────────────────

fn main() {
    if !config_path().exists() {
        save_config(&Config::default());
    }

    let config = Arc::new(Mutex::new(load_config()));

    {
        let cfg = config.lock().unwrap();
        style_all(&cfg);
        set_startup(cfg.run_on_startup);
    }

    // Background sweep — catches new windows as they open
    {
        let config = Arc::clone(&config);
        thread::spawn(move || loop {
            thread::sleep(Duration::from_secs(3));
            style_all(&config.lock().unwrap());
        });
    }

    // Tray menu
    let reload_item = MenuItem::new("Reload config", true, None);
    let folder_item = MenuItem::new("Open config folder", true, None);
    let quit_item   = MenuItem::new("Quit", true, None);
    let reload_id   = reload_item.id().clone();
    let folder_id   = folder_item.id().clone();
    let quit_id     = quit_item.id().clone();

    let menu = Menu::new();
    menu.append(&reload_item).unwrap();
    menu.append(&folder_item).unwrap();
    menu.append(&quit_item).unwrap();

    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("fixmyborderpls")
        .with_icon(make_icon())
        .build()
        .unwrap();

    let menu_rx  = MenuEvent::receiver();
    let _tray_rx = TrayIconEvent::receiver();

    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        loop {
            if let Ok(ev) = menu_rx.try_recv() {
                if ev.id == quit_id {
                    break;
                } else if ev.id == reload_id {
                    let new_cfg = load_config();
                    set_startup(new_cfg.run_on_startup);
                    style_all(&new_cfg);
                    *config.lock().unwrap() = new_cfg;
                } else if ev.id == folder_id {
                    open_config_folder();
                }
            }

            if PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
                if msg.message == WM_QUIT { break; }
            } else {
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
}
