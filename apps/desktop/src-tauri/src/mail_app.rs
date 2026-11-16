//! Regards as the person's default mail app: `mailto:` links on web pages and
//! in other programs open a new message in it (links.rs reads them). Only
//! when the person asks, and each system decides in its own place:
//!
//! - **Windows**: an app cannot make itself the default. Regards declares
//!   that it opens `mailto:` links the way mail apps do, for this user only
//!   (a ProgID, its capabilities, an entry in RegisteredApplications), and
//!   Settings › Default apps opens on Regards' page for the person to choose
//!   it. Whether it took is asked of Windows (which program a click on a link
//!   runs): the person's choice is kept apart from the handlers apps write,
//!   so finding Regards' own handler proves nothing.
//! - **macOS**: chosen in Mail, under Settings › General.
//! - **Linux**: the desktop's handler for `x-scheme-handler/mailto`, set and
//!   read through the deep-link plugin.

use serde::Serialize;
use tauri::AppHandle;

/// Whether a `mailto:` link opens this Regards, and whether it can be asked from here.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct MailAppStatus {
    registered: bool,
    /// It can be asked from Regards; on macOS the person chooses in Mail instead.
    can_register: bool,
}

#[tauri::command]
pub(crate) fn mail_app_status(app: AppHandle) -> MailAppStatus {
    MailAppStatus {
        registered: registered(&app),
        can_register: !cfg!(target_os = "macos"),
    }
}

/// The person asked: make Regards the default mail app. On Windows, Default
/// apps then opens on Regards' page, to choose it there.
#[tauri::command]
pub(crate) fn make_mail_app(app: AppHandle) -> Result<(), String> {
    #[cfg(windows)]
    {
        let _ = app;
        let exe = current_exe().map_err(|error| format!("could not make Regards your mail app: {error}"))?;
        win::declare(&exe).map_err(|error| format!("could not make Regards your mail app: {error}"))?;
        // Regards' own page (Windows 11); where it is not known, the list of default apps.
        let _ = open::that_detached(format!("ms-settings:defaultapps?registeredAppUser={APP_NAME}"));
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        let _ = app;
        Err("on macOS, choose Regards as the default mail app in Mail, under Settings › General".into())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        use tauri_plugin_deep_link::DeepLinkExt;
        app.deep_link()
            .register("mailto")
            .map_err(|error| format!("could not make Regards your mail app: {error}"))
    }
}

fn registered(app: &AppHandle) -> bool {
    #[cfg(windows)]
    {
        let _ = app;
        match (current_exe(), win::mailto_command()) {
            (Ok(exe), Some(command)) => runs(&command, &exe),
            _ => false,
        }
    }
    // Mail keeps the choice; it is not read from here.
    #[cfg(target_os = "macos")]
    {
        let _ = app;
        false
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        use tauri_plugin_deep_link::DeepLinkExt;
        app.deep_link().is_registered("mailto").unwrap_or(false)
    }
}

/// The name Regards is listed under in Default apps.
#[cfg_attr(not(windows), allow(dead_code))]
const APP_NAME: &str = "Regards";
/// What a `mailto:` link is opened as, once the person chose Regards.
#[cfg_attr(not(windows), allow(dead_code))]
const PROG_ID: &str = "Regards.Url.mailto";
#[cfg_attr(not(windows), allow(dead_code))]
const CAPABILITIES: &str = r"Software\Clients\Mail\Regards\Capabilities";

/// What declares Regards a mail app for this user, under HKEY_CURRENT_USER:
/// (key, value name, "" for the key's own value; text). The installer takes
/// them away when Regards is uninstalled (windows/hooks.nsh).
#[cfg_attr(not(windows), allow(dead_code))]
fn entries(exe: &str) -> Vec<(String, &'static str, String)> {
    let prog_id = format!(r"Software\Classes\{PROG_ID}");
    vec![
        (prog_id.clone(), "", "Regards URL".into()),
        (prog_id.clone(), "URL Protocol", String::new()),
        (format!(r"{prog_id}\DefaultIcon"), "", format!("{exe},0")),
        (format!(r"{prog_id}\Application"), "ApplicationName", APP_NAME.into()),
        (format!(r"{prog_id}\Application"), "ApplicationIcon", format!("{exe},0")),
        (
            format!(r"{prog_id}\shell\open\command"),
            "",
            format!("\"{exe}\" \"%1\""),
        ),
        (CAPABILITIES.into(), "ApplicationName", APP_NAME.into()),
        (
            CAPABILITIES.into(),
            "ApplicationDescription",
            "The free, open-source email app built for AI agents.".into(),
        ),
        (format!(r"{CAPABILITIES}\URLAssociations"), "mailto", PROG_ID.into()),
        (r"Software\RegisteredApplications".into(), APP_NAME, CAPABILITIES.into()),
    ]
}

/// Whether a link's command line runs this program (Windows paths: any case).
#[cfg_attr(not(windows), allow(dead_code))]
fn runs(command: &str, exe: &str) -> bool {
    let command = command.trim_start();
    let program = match command.strip_prefix('"') {
        Some(quoted) => quoted.split('"').next().unwrap_or(""),
        None => command.split(' ').next().unwrap_or(""),
    };
    !program.is_empty() && program.eq_ignore_ascii_case(exe)
}

/// This program's path, as Windows writes it in a command line.
#[cfg(windows)]
fn current_exe() -> std::io::Result<String> {
    let exe = std::env::current_exe()?.display().to_string();
    Ok(exe.strip_prefix(r"\\?\").map(str::to_owned).unwrap_or(exe))
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod win {
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW,
        RegSetValueExW,
    };
    use windows_sys::Win32::UI::Shell::{
        ASSOCF_IS_PROTOCOL, ASSOCSTR_COMMAND, AssocQueryStringW, SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify,
    };

    /// Declare Regards a mail app for this user, then tell Windows associations changed.
    pub(super) fn declare(exe: &str) -> std::io::Result<()> {
        for (key, name, text) in super::entries(exe) {
            set(&key, name, &text)?;
        }
        // SAFETY: SHCNE_ASSOCCHANGED takes no items; both are null.
        unsafe {
            SHChangeNotify(
                SHCNE_ASSOCCHANGED as i32,
                SHCNF_IDLIST,
                std::ptr::null(),
                std::ptr::null(),
            );
        }
        Ok(())
    }

    /// One text value under HKEY_CURRENT_USER, its key made if missing.
    fn set(key: &str, name: &str, text: &str) -> std::io::Result<()> {
        let key = wide(key);
        let name = wide(name);
        let text = wide(text);
        let mut handle: HKEY = std::ptr::null_mut();
        // SAFETY: `key` is a NUL-terminated UTF-16 string that outlives the
        // call; the class and security attributes may be null; `handle`
        // receives the opened key.
        let error = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                std::ptr::null(),
                &mut handle,
                std::ptr::null_mut(),
            )
        };
        if error != 0 {
            return Err(std::io::Error::from_raw_os_error(error as i32));
        }
        // SAFETY: `handle` was just opened for setting values; `name` and
        // `text` are NUL-terminated UTF-16 strings, and the size given is
        // `text`'s in bytes, its NUL included, as REG_SZ requires.
        let error = unsafe {
            RegSetValueExW(
                handle,
                name.as_ptr(),
                0,
                REG_SZ,
                text.as_ptr().cast(),
                (text.len() * size_of::<u16>()) as u32,
            )
        };
        // SAFETY: `handle` was opened above and is closed once.
        unsafe {
            RegCloseKey(handle);
        }
        if error != 0 {
            return Err(std::io::Error::from_raw_os_error(error as i32));
        }
        Ok(())
    }

    /// The command a click on a `mailto:` link runs: the person's choice in
    /// Default apps when there is one, as Windows itself decides.
    pub(super) fn mailto_command() -> Option<String> {
        let scheme = wide("mailto");
        let mut buffer = vec![0u16; 2048];
        let mut length = buffer.len() as u32;
        // SAFETY: `scheme` is a NUL-terminated UTF-16 string; `buffer` holds
        // `length` UTF-16 units, which the call writes within and updates.
        let result = unsafe {
            AssocQueryStringW(
                ASSOCF_IS_PROTOCOL,
                ASSOCSTR_COMMAND,
                scheme.as_ptr(),
                std::ptr::null(),
                buffer.as_mut_ptr(),
                &mut length,
            )
        };
        if result != 0 {
            return None;
        }
        let end = buffer.iter().position(|&unit| unit == 0).unwrap_or(buffer.len());
        Some(String::from_utf16_lossy(&buffer[..end]))
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_opens_regards_only_when_its_command_runs_this_program() {
        let exe = r"C:\Users\Ada\AppData\Local\Regards\Regards.exe";
        assert!(runs(r#""C:\Users\Ada\AppData\Local\Regards\Regards.exe" "%1""#, exe));
        assert!(
            runs(r#""c:\users\ada\appdata\local\regards\REGARDS.EXE" "%1""#, exe),
            "Windows paths: any case"
        );
        assert!(runs(
            r"C:\Users\Ada\AppData\Local\Regards\Regards.exe %1",
            r"C:\Users\Ada\AppData\Local\Regards\Regards.exe"
        ));
        for other in [
            r#""C:\Program Files\Mozilla Thunderbird\thunderbird.exe" -osint -compose "%1""#,
            r#""C:\Users\Ada\AppData\Local\Regards\Regards.exe.old" "%1""#,
            r#""C:\Other\Regards.exe" "%1""#,
            "",
        ] {
            assert!(!runs(other, exe), "{other}");
        }
    }

    #[test]
    fn regards_is_declared_a_mail_app_the_way_windows_lists_them() {
        let exe = r"C:\R\Regards.exe";
        let entries = entries(exe);
        let find = |key: &str, name: &str| {
            entries
                .iter()
                .find(|(k, n, _)| k == key && *n == name)
                .map(|(_, _, text)| text.as_str())
        };
        assert_eq!(
            find(r"Software\Classes\Regards.Url.mailto\shell\open\command", ""),
            Some(r#""C:\R\Regards.exe" "%1""#),
            "the link reaches Regards as one argument, which links.rs takes"
        );
        assert_eq!(
            find(r"Software\RegisteredApplications", "Regards"),
            Some(r"Software\Clients\Mail\Regards\Capabilities")
        );
        assert_eq!(
            find(r"Software\Clients\Mail\Regards\Capabilities\URLAssociations", "mailto"),
            Some("Regards.Url.mailto")
        );
        assert!(
            entries.iter().all(|(key, ..)| key.starts_with(r"Software\")),
            "for this user only, never the plain mailto key other apps share"
        );
        assert!(
            !entries
                .iter()
                .any(|(key, ..)| key.eq_ignore_ascii_case(r"Software\Classes\mailto"))
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_says_which_program_opens_mailto_links() {
        // Some machines have none; asking never fails loudly.
        let _ = win::mailto_command();
    }
}
