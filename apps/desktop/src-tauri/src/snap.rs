//! Windows 11's Snap Layouts on the app's own maximise button. The window
//! draws its own caption buttons over a WebView2, so Windows never learns
//! where the maximise button is and cannot offer its layouts on hover.
//! Hovering ours a moment asks for them as the keyboard does (Win+Z), when
//! Windows has them, shows them on hover (Settings › System › Multitasking),
//! and this window is the one in front.

use tauri::WebviewWindow;

/// The maximise button was hovered a moment: offer the layouts (on
/// Windows; elsewhere there are none).
#[tauri::command]
pub(crate) fn snap_layouts(window: WebviewWindow) {
    #[cfg(windows)]
    if window.is_focused().unwrap_or(false) && win::offered() {
        win::press_win_z();
    }
    #[cfg(not(windows))]
    let _ = window;
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod win {
    use windows_sys::Wdk::System::SystemServices::RtlGetVersion;
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY, VK_LWIN,
    };

    /// Windows 11 (build 22000 and later) with the layouts shown on hover.
    pub(super) fn offered() -> bool {
        build() >= 22_000 && hover_setting().unwrap_or(1) != 0
    }

    pub(super) fn build() -> u32 {
        let mut info = OSVERSIONINFOW {
            dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32,
            ..Default::default()
        };
        // SAFETY: `info` is a valid OSVERSIONINFOW whose size field is set,
        // as RtlGetVersion requires; it only writes into it.
        let status = unsafe { RtlGetVersion(&mut info) };
        if status == 0 { info.dwBuildNumber } else { 0 }
    }

    /// "Show snap layouts when I hover over a window's maximise button":
    /// absent means on.
    pub(super) fn hover_setting() -> Option<u32> {
        let key = wide(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced");
        let value = wide("EnableSnapAssistFlyout");
        let mut data = 0u32;
        let mut size = size_of::<u32>() as u32;
        // SAFETY: both names are NUL-terminated UTF-16 strings that outlive
        // the call; `data` and `size` describe a 4-byte buffer for a DWORD.
        let error = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                (&raw mut data).cast(),
                &mut size,
            )
        };
        (error == 0).then_some(data)
    }

    /// Win+Z: "open snap layouts" for the window in front.
    pub(super) fn press_win_z() {
        const Z: VIRTUAL_KEY = 0x5A;
        let key = |vk: VIRTUAL_KEY, up: bool| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let inputs = [key(VK_LWIN, false), key(Z, false), key(Z, true), key(VK_LWIN, true)];
        // SAFETY: `inputs` is an array of fully initialised keyboard INPUTs
        // that outlives the call, and its length and element size are given.
        unsafe {
            SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32);
        }
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn windows_version_and_setting_are_read() {
        // Windows 10 or 11: a build number is always there.
        assert!(super::win::build() >= 10_240);
        // The setting is a DWORD when present; reading it never fails loudly.
        let _ = super::win::hover_setting();
    }
}
