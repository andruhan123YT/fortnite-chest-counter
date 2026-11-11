//! Small OS integrations: single instance, console output for CLI use, autostart, toast
//! notifications and the desktop shortcut. Everything is a no-op outside Windows.

use crate::APP_NAME;

#[cfg(windows)]
mod imp {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    use windows_sys::Win32::System::Threading::{
        CreateEventW, CreateMutexW, OpenEventW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE, INFINITE,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW,
    };

    use super::APP_NAME;

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    pub fn attach_parent_console() {
        // SAFETY: plain Win32 call without pointers.
        unsafe {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }

    const INSTANCE_MUTEX: &str = "Local\\FortniteChestCounter.SingleInstance";
    const ACTIVATE_EVENT: &str = "Local\\FortniteChestCounter.Activate";

    /// Returns true if another instance already runs. That instance is asked to show its
    /// window, which also works while it sits hidden in the tray.
    pub fn activate_existing_instance() -> bool {
        let mutex_name = wide(INSTANCE_MUTEX);
        // SAFETY: the name is a valid null-terminated UTF-16 string that outlives the call.
        // The mutex handle is intentionally leaked: it must live as long as the process.
        let already_running = unsafe {
            CreateMutexW(std::ptr::null(), 0, mutex_name.as_ptr());
            GetLastError() == ERROR_ALREADY_EXISTS
        };
        if already_running {
            let event_name = wide(ACTIVATE_EVENT);
            // SAFETY: as above; a null handle is checked before use.
            unsafe {
                let event = OpenEventW(EVENT_MODIFY_STATE, 0, event_name.as_ptr());
                if event.is_null() {
                    show_main_window();
                } else {
                    SetEvent(event);
                }
            }
        }
        already_running
    }

    /// Becomes the single instance once the running one has quit, waiting up to `max`.
    pub fn claim_instance_waiting(max: std::time::Duration) -> bool {
        use windows_sys::Win32::Foundation::CloseHandle;
        let mutex_name = wide(INSTANCE_MUTEX);
        let deadline = std::time::Instant::now() + max;
        loop {
            // SAFETY: as in `activate_existing_instance`. A handle to a mutex someone else
            // still holds is closed again; the one that wins is kept for the process.
            // No handle at all means the call failed: that is no claim either.
            let claimed = unsafe {
                let handle = CreateMutexW(std::ptr::null(), 0, mutex_name.as_ptr());
                let taken = GetLastError() == ERROR_ALREADY_EXISTS;
                if taken && !handle.is_null() {
                    CloseHandle(handle);
                }
                !handle.is_null() && !taken
            };
            if claimed {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }

    /// Calls `on_activate` whenever a second instance asks this one to show itself.
    pub fn listen_for_activation(on_activate: impl Fn() + Send + 'static) {
        let event_name = wide(ACTIVATE_EVENT);
        // SAFETY: as above. The auto-reset event lives for the rest of the process.
        let event = unsafe { CreateEventW(std::ptr::null(), 0, 0, event_name.as_ptr()) };
        if event.is_null() {
            return;
        }
        let event = event as usize;
        std::thread::spawn(move || loop {
            // SAFETY: the handle stays valid because it is never closed.
            if unsafe { WaitForSingleObject(event as _, INFINITE) } != WAIT_OBJECT_0 {
                return;
            }
            on_activate();
        });
    }

    /// Opens Explorer on the folder of `path` with the file selected.
    pub fn reveal_in_explorer(path: &std::path::Path) -> std::io::Result<()> {
        use std::os::windows::process::CommandExt;
        // Explorer reads `/select,` only outside the quotes, and the usual argument
        // quoting wraps all of it once the path has a space, so Explorer opens Documents
        // instead. Quote just the path.
        std::process::Command::new("explorer").raw_arg(super::select_arg(path)).spawn().map(drop)
    }

    /// Opens a web address in the default browser.
    pub fn open_url(url: &str) -> std::io::Result<()> {
        std::process::Command::new("explorer").arg(url).spawn().map(drop)
    }

    pub fn show_main_window() {
        let title = wide(APP_NAME);
        // SAFETY: `title` is a valid null-terminated UTF-16 string; a null class matches any.
        unsafe {
            let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
            if !hwnd.is_null() {
                ShowWindow(hwnd, SW_SHOW);
                ShowWindow(hwnd, SW_RESTORE);
                SetForegroundWindow(hwnd);
            }
        }
    }

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

    pub fn set_autostart(enabled: bool) -> std::io::Result<()> {
        use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
        let key = winreg::RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE)?;
        if enabled {
            let exe = std::env::current_exe()?;
            key.set_value(APP_NAME, &format!("\"{}\" --minimized", exe.display()))
        } else {
            match key.delete_value(APP_NAME) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                other => other,
            }
        }
    }

    /// The id notifications are sent under, registered by `register_notifications`.
    const APP_USER_MODEL_ID: &str = "FortniteChestCounter";
    static NOTIFY_AS_APP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    /// Gives notifications the app's name and icon. Windows only shows notifications from
    /// apps it knows: a desktop app without an installer becomes known through this key,
    /// otherwise its notifications have to borrow PowerShell's id.
    pub fn register_notifications(icon: &std::path::Path) -> std::io::Result<()> {
        use winreg::enums::HKEY_CURRENT_USER;
        let key_path = format!(r"Software\Classes\AppUserModelId\{APP_USER_MODEL_ID}");
        let (key, _) = winreg::RegKey::predef(HKEY_CURRENT_USER).create_subkey(key_path)?;
        key.set_value("DisplayName", &APP_NAME)?;
        key.set_value("IconUri", &icon.display().to_string())?;
        NOTIFY_AS_APP.store(true, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    pub fn notify(title: &str, body: &str) {
        use tauri_winrt_notification::Toast;
        let show = |id: &str| Toast::new(id).title(title).text1(body).show();
        if !NOTIFY_AS_APP.load(std::sync::atomic::Ordering::Relaxed) || show(APP_USER_MODEL_ID).is_err() {
            let _ = show(Toast::POWERSHELL_APP_ID);
        }
    }

    /// Puts a shortcut to this executable on the desktop, unless one is there already.
    pub fn create_desktop_shortcut() -> std::io::Result<std::path::PathBuf> {
        let desktop = dirs::desktop_dir().ok_or_else(|| std::io::Error::other("no desktop folder"))?;
        let link_path = desktop.join(format!("{APP_NAME}.lnk"));
        if !link_path.exists() {
            create_shortcut(&link_path, &std::env::current_exe()?)?;
        }
        Ok(link_path)
    }

    /// Writes a Windows shortcut at `link_path` that starts `target`.
    pub fn create_shortcut(link_path: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
        use windows::core::{Interface, HSTRING};
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
            COINIT_APARTMENTTHREADED,
        };
        use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

        let folder = target.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        // SAFETY: COM is set up on this thread for the calls below and released after;
        // every interface is used only while alive and dropped before `CoUninitialize`.
        unsafe {
            let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
            let saved = (|| -> windows::core::Result<()> {
                let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
                link.SetPath(&HSTRING::from(target.as_os_str()))?;
                link.SetWorkingDirectory(&HSTRING::from(folder.as_os_str()))?;
                link.SetDescription(&HSTRING::from("Counts the Fortnite chests you open and break"))?;
                link.cast::<IPersistFile>()?.Save(&HSTRING::from(link_path.as_os_str()), true)
            })();
            if initialized {
                CoUninitialize();
            }
            saved.map_err(std::io::Error::other)
        }
    }

    /// Primary monitor size in physical pixels.
    pub fn primary_screen_size() -> Option<(f32, f32)> {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
        // SAFETY: plain Win32 calls without pointers.
        let (w, h) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
        (w > 0 && h > 0).then_some((w as f32, h as f32))
    }

    const FORTNITE_PROCESS: &str = "FortniteClient-Win64-Shipping.exe";

    pub fn fortnite_running() -> bool {
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
        };
        // SAFETY: the snapshot handle is checked and closed; the entry struct is sized as
        // the API requires and only read after a successful call.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return false;
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut found = false;
            let mut ok = Process32FirstW(snapshot, &mut entry) != 0;
            while ok {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                if String::from_utf16_lossy(&entry.szExeFile[..len]).eq_ignore_ascii_case(FORTNITE_PROCESS) {
                    found = true;
                    break;
                }
                ok = Process32NextW(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
            found
        }
    }

    /// Makes a top-level window translucent. Returns false if the window is not there yet.
    pub fn set_window_opacity(title: &str, alpha: u8) -> bool {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetWindowLongW, SetLayeredWindowAttributes, SetWindowLongW, GWL_EXSTYLE, LWA_ALPHA, WS_EX_LAYERED,
        };
        let title = wide(title);
        // SAFETY: `title` is a valid null-terminated UTF-16 string; the handle is checked.
        unsafe {
            let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
            if hwnd.is_null() {
                return false;
            }
            let style = GetWindowLongW(hwnd, GWL_EXSTYLE);
            SetWindowLongW(hwnd, GWL_EXSTYLE, style | WS_EX_LAYERED as i32);
            SetLayeredWindowAttributes(hwnd, 0, alpha, LWA_ALPHA) != 0
        }
    }

    /// Turns off the animation Windows plays when a top-level window is shown. Returns
    /// false if the window is not there yet.
    pub fn disable_window_animation(title: &str) -> bool {
        use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_TRANSITIONS_FORCEDISABLED};
        let title = wide(title);
        let disabled: i32 = 1;
        // SAFETY: `title` is a valid null-terminated UTF-16 string; the handle is checked;
        // the attribute value is a BOOL that lives through the call.
        unsafe {
            let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
            if hwnd.is_null() {
                return false;
            }
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_TRANSITIONS_FORCEDISABLED as u32,
                (&disabled as *const i32).cast(),
                size_of::<i32>() as u32,
            );
            true
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn attach_parent_console() {}
    pub fn listen_for_activation(_on_activate: impl Fn() + Send + 'static) {}
    pub fn activate_existing_instance() -> bool {
        false
    }
    pub fn claim_instance_waiting(_max: std::time::Duration) -> bool {
        true
    }
    pub fn reveal_in_explorer(_path: &std::path::Path) -> std::io::Result<()> {
        Err(std::io::Error::other("only on Windows"))
    }
    #[allow(dead_code)]
    pub fn show_main_window() {}
    pub fn set_autostart(_enabled: bool) -> std::io::Result<()> {
        Ok(())
    }
    pub fn register_notifications(_icon: &std::path::Path) -> std::io::Result<()> {
        Ok(())
    }
    pub fn notify(title: &str, body: &str) {
        eprintln!("[{}] {title}: {body}", super::APP_NAME);
    }
    pub fn create_desktop_shortcut() -> std::io::Result<std::path::PathBuf> {
        Err(std::io::Error::other("only on Windows"))
    }
    pub fn open_url(_url: &str) -> std::io::Result<()> {
        Err(std::io::Error::other("only on Windows"))
    }
    pub fn primary_screen_size() -> Option<(f32, f32)> {
        None
    }
    pub fn fortnite_running() -> bool {
        false
    }
    pub fn set_window_opacity(_title: &str, _alpha: u8) -> bool {
        false
    }
    pub fn disable_window_animation(_title: &str) -> bool {
        true
    }
}

pub use imp::*;

/// The Explorer argument that selects `path`, quoted so spaces survive.
#[cfg_attr(not(windows), allow(dead_code))]
fn select_arg(path: &std::path::Path) -> String {
    format!("/select,\"{}\"", path.display())
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    #[test]
    fn writes_a_shortcut() {
        let dir = std::env::temp_dir().join(format!("fcc-shortcut-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let link = dir.join("test.lnk");
        super::create_shortcut(&link, &std::env::current_exe().unwrap()).unwrap();
        // Every shortcut file starts with its header size, 0x4C.
        assert_eq!(std::fs::read(&link).unwrap()[..4], [0x4C, 0, 0, 0]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn select_keeps_the_switch_outside_the_quotes() {
        let path = std::path::Path::new(r"C:\Roaming\Fortnite Chest Counter\Epic profile\get-epic-profile.bat");
        assert_eq!(
            super::select_arg(path),
            r#"/select,"C:\Roaming\Fortnite Chest Counter\Epic profile\get-epic-profile.bat""#
        );
    }
}
