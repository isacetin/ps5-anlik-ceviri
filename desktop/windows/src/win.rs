//! Pencere listeleme ve sistem bilgisi (Win32).

use windows::core::{BOOL, PWSTR};
use windows::Wdk::System::SystemServices::RtlGetVersion;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::{ClientToScreen, GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
};

#[derive(Clone, Debug)]
pub struct WindowInfo {
    pub hwnd: HWND,
    pub title: String,
    pub exe: String,
    /// DWM'in çizdiği gerçek pencere sınırı (fiziksel piksel); yakalanan kare bu boyuttadır.
    pub frame: RECT,
    /// Pencerenin üstünden istemci alanına kadar olan yükseklik (başlık çubuğu), piksel.
    pub top_inset: i32,
    pub minimized: bool,
}

impl WindowInfo {
    pub fn width(&self) -> i32 {
        self.frame.right - self.frame.left
    }

    pub fn height(&self) -> i32 {
        self.frame.bottom - self.frame.top
    }

    /// PS Remote Play penceresi mi? (exe: RemotePlay.exe, başlık: "PS Remote Play")
    pub fn looks_like_remote_play(&self) -> bool {
        let exe = self.exe.to_lowercase();
        let title = self.title.to_lowercase();
        exe.contains("remoteplay") || title.contains("remote play") || title.contains("uzaktan oynama")
    }
}

/// Pencerenin bulunduğu monitörün ölçeği (100% = 1.0, 150% = 1.5): fiziksel piksel / mantıksal birim.
pub fn scale_factor(window: &WindowInfo) -> f64 {
    let dpi = unsafe { GetDpiForWindow(window.hwnd) };
    if dpi == 0 {
        1.0
    } else {
        dpi as f64 / 96.0
    }
}

/// Pencere bulunduğu monitörü tamamen kaplıyorsa (başlık çubuğu yok) tam ekrandır.
pub fn is_fullscreen(window: &WindowInfo) -> bool {
    unsafe {
        let monitor = MonitorFromWindow(window.hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return false;
        }
        let screen = info.rcMonitor;
        window.frame.left <= screen.left && window.frame.top <= screen.top && window.frame.right >= screen.right && window.frame.bottom >= screen.bottom
    }
}

/// Koordinatların ölçeklenmeden fiziksel piksel olarak gelmesi için en başta çağrılır.
pub fn enable_dpi_awareness() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

pub fn os_version() -> String {
    let mut info = OSVERSIONINFOW { dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32, ..Default::default() };
    unsafe {
        let _ = RtlGetVersion(&mut info);
    }
    let name = if info.dwBuildNumber >= 22000 { "Windows 11" } else { "Windows 10" };
    format!("{name} ({}.{}.{})", info.dwMajorVersion, info.dwMinorVersion, info.dwBuildNumber)
}

/// Ekranda görünen, gizlenmemiş, 100 px'den büyük üst düzey pencereler.
pub fn visible_windows() -> Vec<WindowInfo> {
    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let list = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
        list.push(hwnd);
        true.into()
    }

    let mut handles: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut handles as *mut _ as isize));
    }
    handles.into_iter().filter_map(window_info).collect()
}

fn window_info(hwnd: HWND) -> Option<WindowInfo> {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            return None;
        }
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut _ as _, 4);
        if cloaked != 0 {
            return None;
        }
        let mut title = [0u16; 512];
        let len = GetWindowTextW(hwnd, &mut title);
        let title = String::from_utf16_lossy(&title[..len.max(0) as usize]);

        let mut frame = RECT::default();
        DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &mut frame as *mut _ as _, std::mem::size_of::<RECT>() as u32).ok()?;
        let minimized = IsIconic(hwnd).as_bool();
        if !minimized && (frame.right - frame.left < 100 || frame.bottom - frame.top < 100) {
            return None;
        }

        // İstemci alanının sol üst köşesi ekranda nerede: pencere üstüyle farkı başlık çubuğudur.
        let mut origin = POINT::default();
        let top_inset = if ClientToScreen(hwnd, &mut origin).as_bool() {
            (origin.y - frame.top).max(0)
        } else {
            0
        };

        Some(WindowInfo { hwnd, title, exe: process_name(hwnd), frame, top_inset, minimized })
    }
}

fn process_name(hwnd: HWND) -> String {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return "?".into();
        };
        let mut buffer = [0u16; 1024];
        let mut size = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buffer.as_mut_ptr()), &mut size).is_ok();
        let _ = CloseHandle(process);
        if !ok {
            return "?".into();
        }
        let path = String::from_utf16_lossy(&buffer[..size as usize]);
        path.rsplit('\\').next().unwrap_or(&path).to_string()
    }
}
