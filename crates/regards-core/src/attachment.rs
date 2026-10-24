//! Received attachments: the name to show and to save under, whether the app
//! can show one by itself, and how risky it is to open. Names and declared
//! types come from the sender and are not trusted: a preview follows the
//! file's own bytes, and the risk follows the extension the system acts on.

use serde::{Deserialize, Serialize};

/// How an attachment may be opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    /// Opens in its app like any document.
    Open,
    /// Can run scripts or macros, or carry other files (web pages, SVG,
    /// macro-enabled Office files, archives): the person confirms first.
    Warn,
    /// A program, script or shortcut, or a name dressed up as something
    /// else: it can be saved, not opened from Regards.
    SaveOnly,
}

/// What the app can show of an attachment by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Preview {
    Image,
    Text,
}

/// Longest name kept when saving, extension included.
const MAX_NAME: usize = 150;

/// What Outlook blocks (support.microsoft.com, "Blocked attachments in
/// Outlook"), with the package, disk image and launcher types of Windows,
/// macOS and Linux.
const SAVE_ONLY: &[&str] = &[
    "ade",
    "adp",
    "app",
    "appcontent-ms",
    "application",
    "appref-ms",
    "appimage",
    "appx",
    "appxbundle",
    "asp",
    "aspx",
    "asx",
    "bas",
    "bash",
    "bat",
    "bgi",
    "cab",
    "cer",
    "chm",
    "cmd",
    "cnt",
    "com",
    "command",
    "cpl",
    "crt",
    "csh",
    "deb",
    "der",
    "desktop",
    "diagcab",
    "dmg",
    "exe",
    "fxp",
    "gadget",
    "grp",
    "hlp",
    "hpj",
    "hta",
    "htc",
    "img",
    "inf",
    "ins",
    "iso",
    "isp",
    "its",
    "jar",
    "jnlp",
    "js",
    "jse",
    "ksh",
    "library-ms",
    "lnk",
    "mad",
    "maf",
    "mag",
    "mam",
    "maq",
    "mar",
    "mas",
    "mat",
    "mau",
    "mav",
    "maw",
    "mcf",
    "mda",
    "mdb",
    "mde",
    "mdt",
    "mdw",
    "mdz",
    "mht",
    "mhtml",
    "mpkg",
    "msc",
    "msh",
    "msh1",
    "msh1xml",
    "msh2",
    "msh2xml",
    "mshxml",
    "msi",
    "msix",
    "msixbundle",
    "msp",
    "mst",
    "msu",
    "ops",
    "osd",
    "pcd",
    "pif",
    "pkg",
    "pl",
    "plg",
    "prf",
    "prg",
    "printerexport",
    "ps1",
    "ps1xml",
    "ps2",
    "ps2xml",
    "psc1",
    "psc2",
    "psd1",
    "psdm1",
    "pst",
    "py",
    "pyc",
    "pyo",
    "pyw",
    "pyz",
    "pyzw",
    "reg",
    "rpm",
    "run",
    "scf",
    "scpt",
    "scr",
    "sct",
    "search-ms",
    "searchconnector-ms",
    "settingcontent-ms",
    "sh",
    "shb",
    "shs",
    "terminal",
    "theme",
    "tool",
    "url",
    "vb",
    "vbe",
    "vbp",
    "vbs",
    "vhd",
    "vhdx",
    "vsmacros",
    "vsw",
    "webloc",
    "webpnp",
    "website",
    "workflow",
    "ws",
    "wsb",
    "wsc",
    "wsf",
    "wsh",
    "xbap",
    "xll",
    "xnk",
    "zsh",
];

/// Opened only after a confirmation, as is a file without an extension.
const WARN: &[&str] = &[
    "7z", "ace", "arj", "bz2", "docm", "dotm", "gz", "htm", "html", "lzh", "potm", "ppam", "ppsm", "pptm", "rar",
    "shtml", "sldm", "svg", "svgz", "tar", "tgz", "xht", "xhtml", "xlam", "xlsm", "xltm", "xz", "zip", "zst",
];

/// Shown as text, as a text node: never interpreted.
const TEXT: &[&str] = &[
    "cfg", "conf", "csv", "eml", "htm", "html", "ics", "ini", "json", "log", "markdown", "md", "text", "tsv", "txt",
    "vcf", "xml", "yaml", "yml",
];

/// Image extensions a sniffed image may carry.
const IMAGE: &[&str] = &["gif", "jfif", "jpe", "jpeg", "jpg", "png", "webp"];

/// Characters that change the direction of text: `invoice\u{202e}fdp.exe`
/// reads as `invoiceexe.pdf`.
fn is_bidi_control(c: char) -> bool {
    matches!(
        c,
        '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{200e}' | '\u{200f}' | '\u{061c}'
    )
}

/// The name to show: the sender's, without any folder, control or
/// direction characters. `None` when nothing is left.
pub fn display_name(name: &str) -> Option<String> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let clean: String = base
        .chars()
        .filter(|c| !c.is_control() && !is_bidi_control(*c))
        .collect();
    let clean = clean.trim();
    (!clean.is_empty()).then(|| clean.to_owned())
}

/// A file's name from the parameters a message's structure gives its part
/// (BODYSTRUCTURE): Content-Disposition `filename`, else Content-Type
/// `name`. Split or percent-encoded names (RFC 2231) and encoded words (RFC
/// 2047) are put back together as when the message itself is read, then
/// cleaned as [`display_name`]. `None` if it has none.
pub fn name_from_parameters(params: &[(String, String)]) -> Option<String> {
    // mail-parser decodes them: give it the parameters as header lines,
    // without anything that could end a line or a parameter early.
    let render = |wanted: &str| -> String {
        params
            .iter()
            .filter(|(key, _)| key == wanted || key.strip_prefix(wanted).is_some_and(|rest| rest.starts_with('*')))
            .filter(|(key, _)| {
                key.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '*' | '-' | '_'))
            })
            .map(|(key, value)| {
                let value: String = value.chars().filter(|c| !c.is_control()).collect();
                if key.ends_with('*') {
                    format!("; {key}={}", value.replace([';', ' ', '"'], ""))
                } else {
                    format!("; {key}=\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
                }
            })
            .collect()
    };
    let header = format!(
        "Content-Type: application/octet-stream{}\r\nContent-Disposition: attachment{}\r\n\r\n",
        render("name"),
        render("filename")
    );
    let message = mail_parser::MessageParser::default().parse(header.as_bytes())?;
    let name = mail_parser::MimeHeaders::attachment_name(message.root_part())?;
    display_name(name)
}

/// The name to save under, safe on every system: [`display_name`] without
/// reserved characters, leading or trailing dots and spaces, or device names
/// (`CON`, `NUL.txt`…), at most 150 characters with the extension kept. An
/// unnamed file gets `attachment` and an extension for its declared type,
/// only if that type is harmless.
pub fn file_name(name: Option<&str>, mime: &str) -> String {
    let shown = name.and_then(display_name).unwrap_or_default();
    let clean: String = shown
        .chars()
        .filter(|c| !matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'))
        .collect();
    let clean = clean.trim_matches(|c: char| c == '.' || c.is_whitespace());
    let mut clean = if clean.is_empty() {
        match extension_for(mime) {
            Some(extension) => format!("attachment.{extension}"),
            None => "attachment".to_owned(),
        }
    } else {
        clean.to_owned()
    };
    let stem = clean.split('.').next().unwrap_or_default().trim_end();
    if is_device_name(stem) {
        clean.insert(0, '_');
    }
    shorten(&clean)
}

/// Windows reserves these names, with any extension.
fn is_device_name(stem: &str) -> bool {
    let upper = stem.to_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
        || ((upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.chars().count() == 4
            && upper
                .chars()
                .nth(3)
                .is_some_and(|c| matches!(c, '0'..='9' | '¹' | '²' | '³')))
}

fn shorten(name: &str) -> String {
    if name.chars().count() <= MAX_NAME {
        return name.to_owned();
    }
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) if extension.chars().count() <= 16 => (stem, format!(".{extension}")),
        _ => (name, String::new()),
    };
    let keep = MAX_NAME - extension.chars().count();
    let stem: String = stem.chars().take(keep).collect();
    format!(
        "{}{extension}",
        stem.trim_end_matches(|c: char| c == '.' || c.is_whitespace())
    )
}

/// An extension for an unnamed file of a harmless declared type.
fn extension_for(mime: &str) -> Option<&'static str> {
    Some(match mime {
        "application/pdf" => "pdf",
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "text/plain" => "txt",
        "text/csv" => "csv",
        "text/calendar" => "ics",
        "text/vcard" | "text/x-vcard" => "vcf",
        "message/rfc822" => "eml",
        _ => return None,
    })
}

/// The extension the system acts on: the last one, lowercase.
pub fn extension(file_name: &str) -> Option<String> {
    file_name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_lowercase())
        .filter(|extension| !extension.is_empty())
}

/// How risky the attachment is to open, from the name the sender gave it.
pub fn risk(name: Option<&str>, mime: &str) -> Risk {
    // A name that hides its real end is treated as the program it may be.
    if name.is_some_and(|name| name.chars().any(is_bidi_control)) {
        return Risk::SaveOnly;
    }
    match extension(&file_name(name, mime)) {
        Some(extension) if SAVE_ONLY.contains(&extension.as_str()) => Risk::SaveOnly,
        Some(extension) if WARN.contains(&extension.as_str()) => Risk::Warn,
        Some(_) => Risk::Open,
        // Nothing says what it is.
        None => Risk::Warn,
    }
}

/// The image format of `data`, from its first bytes, as a media type.
pub fn image_type(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if data.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if data.len() >= 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        Some("image/webp")
    } else if looks_like_svg(data) {
        Some("image/svg+xml")
    } else {
        None
    }
}

/// An SVG document: XML whose start names an `<svg>` element.
fn looks_like_svg(data: &[u8]) -> bool {
    let start = String::from_utf8_lossy(&data[..data.len().min(4096)]);
    let start = start.trim_start_matches('\u{feff}').trim_start();
    start.starts_with('<') && start.contains("<svg")
}

/// What the app can show of an attachment by itself: an image when its bytes
/// are one and its name agrees, text when its name says so and its bytes
/// hold no NUL.
pub fn preview(name: Option<&str>, mime: &str, data: &[u8]) -> Option<Preview> {
    let extension = extension(&file_name(name, mime));
    let extension = extension.as_deref();
    match image_type(data) {
        Some("image/svg+xml") if extension == Some("svg") || (extension.is_none() && mime == "image/svg+xml") => {
            return Some(Preview::Image);
        }
        Some(kind) if kind != "image/svg+xml" && extension.is_none_or(|e| IMAGE.contains(&e)) => {
            return Some(Preview::Image);
        }
        _ => {}
    }
    let texty = match extension {
        Some(extension) => TEXT.contains(&extension),
        None => matches!(mime, "text/plain" | "text/csv"),
    };
    (texty && !data[..data.len().min(8192)].contains(&0)).then_some(Preview::Text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(params: &[(&str, &str)]) -> Option<String> {
        let params: Vec<(String, String)> = params.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
        name_from_parameters(&params)
    }

    #[test]
    fn a_files_name_from_its_structure_is_decoded_as_when_read() {
        assert_eq!(named(&[("name", "Ticket.pdf")]).as_deref(), Some("Ticket.pdf"));
        assert_eq!(
            named(&[("name", "x.bin"), ("filename", "Rapport annuel.pdf")]).as_deref(),
            Some("Rapport annuel.pdf"),
            "the disposition's name first"
        );
        assert_eq!(
            named(&[("filename*", "utf-8''Z%C3%BCrich%20Lugano.pdf")]).as_deref(),
            Some("Zürich Lugano.pdf"),
            "RFC 2231"
        );
        assert_eq!(
            named(&[("filename*0*", "utf-8''Fa%C3%A7ture"), ("filename*1", " 2026.pdf")]).as_deref(),
            Some("Façture 2026.pdf"),
            "RFC 2231, split"
        );
        assert_eq!(
            named(&[("name", "=?utf-8?Q?Re=C3=A7u_octobre.pdf?=")]).as_deref(),
            Some("Reçu octobre.pdf"),
            "RFC 2047"
        );
        assert_eq!(
            named(&[("filename", "..\\..\\notes.txt\r\nX-Evil: 1")]).as_deref(),
            Some("notes.txtX-Evil: 1"),
            "no folders, and the line stays one parameter"
        );
        assert_eq!(named(&[("charset", "utf-8")]), None);
    }

    #[test]
    fn names_lose_folders_controls_and_direction_marks() {
        assert_eq!(display_name("../../etc/passwd").as_deref(), Some("passwd"));
        assert_eq!(display_name("C:\\Users\\x\\report.pdf").as_deref(), Some("report.pdf"));
        assert_eq!(
            display_name("invoice\u{202e}fdp.exe").as_deref(),
            Some("invoicefdp.exe")
        );
        assert_eq!(display_name("a\u{0}b\r\n.txt").as_deref(), Some("ab.txt"));
        assert_eq!(display_name(" \u{202e} "), None);
    }

    #[test]
    fn file_names_are_safe_everywhere() {
        assert_eq!(file_name(Some("re: plan? <v2>.txt"), "text/plain"), "re plan v2.txt");
        assert_eq!(file_name(Some("...hidden.txt.. "), "text/plain"), "hidden.txt");
        assert_eq!(file_name(Some("nul.txt"), "text/plain"), "_nul.txt");
        assert_eq!(file_name(Some("COM1"), "text/plain"), "_COM1");
        assert_eq!(file_name(Some("com¹.log"), "text/plain"), "_com¹.log");
        assert_eq!(file_name(Some("computer.txt"), "text/plain"), "computer.txt");
        assert_eq!(file_name(None, "application/pdf"), "attachment.pdf");
        assert_eq!(file_name(Some("///"), "application/x-msdownload"), "attachment");
        let long = format!("{}.pdf", "a".repeat(400));
        let short = file_name(Some(&long), "application/pdf");
        assert_eq!(short.chars().count(), MAX_NAME);
        assert!(short.ends_with("a.pdf"));
    }

    #[test]
    fn risk_follows_the_last_extension_and_disguises() {
        assert_eq!(risk(Some("report.pdf"), "application/pdf"), Risk::Open);
        assert_eq!(risk(Some("photo.JPG"), "image/jpeg"), Risk::Open);
        assert_eq!(risk(Some("invoice.pdf.exe"), "application/pdf"), Risk::SaveOnly);
        assert_eq!(risk(Some("setup.exe. . "), "application/pdf"), Risk::SaveOnly);
        assert_eq!(risk(Some("invoice\u{202e}fdp.txt"), "text/plain"), Risk::SaveOnly);
        assert_eq!(risk(Some("run.PS1"), "text/plain"), Risk::SaveOnly);
        assert_eq!(risk(Some("page.html"), "text/html"), Risk::Warn);
        assert_eq!(risk(Some("budget.xlsm"), "application/vnd.ms-excel"), Risk::Warn);
        assert_eq!(risk(Some("photos.zip"), "application/zip"), Risk::Warn);
        assert_eq!(risk(None, "application/pdf"), Risk::Open);
        assert_eq!(risk(None, "application/x-msdownload"), Risk::Warn);
    }

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

    #[test]
    fn images_are_previewed_by_their_bytes_when_the_name_agrees() {
        assert_eq!(preview(Some("a.png"), "image/png", PNG), Some(Preview::Image));
        assert_eq!(
            preview(Some("a.jpg"), "application/octet-stream", PNG),
            Some(Preview::Image)
        );
        assert_eq!(preview(None, "application/octet-stream", PNG), Some(Preview::Image));
        // Declared an image, but it is not one; or an image named as something else.
        assert_eq!(preview(Some("a.png"), "image/png", b"MZ\x90\0"), None);
        assert_eq!(preview(Some("a.exe"), "image/png", PNG), None);
        assert_eq!(preview(Some("a.gif"), "image/gif", b"GIF89a.."), Some(Preview::Image));
        assert_eq!(
            preview(Some("a.webp"), "image/webp", b"RIFF\0\0\0\0WEBPVP8 "),
            Some(Preview::Image)
        );
        let svg = b"<?xml version=\"1.0\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\"/>";
        assert_eq!(preview(Some("logo.svg"), "image/svg+xml", svg), Some(Preview::Image));
        assert_eq!(preview(Some("logo.html"), "text/html", svg), Some(Preview::Text));
    }

    #[test]
    fn text_is_previewed_when_named_so_and_free_of_nul() {
        assert_eq!(preview(Some("notes.txt"), "text/plain", b"hello"), Some(Preview::Text));
        assert_eq!(
            preview(Some("data.CSV"), "application/octet-stream", b"a,b"),
            Some(Preview::Text)
        );
        assert_eq!(preview(None, "text/plain", b"hello"), Some(Preview::Text));
        assert_eq!(preview(Some("notes.txt"), "text/plain", b"he\0llo"), None);
        assert_eq!(preview(Some("report.pdf"), "application/pdf", b"%PDF-1.7"), None);
        assert_eq!(preview(Some("script.js"), "text/javascript", b"alert(1)"), None);
    }
}
