//! "Received" notifications. The XML is built here (pure, tested); showing it is per platform.
//!
//! Windows: an unpackaged app may raise toasts once its `AppUserModelID` has a `DisplayName` under
//! `HKCU\Software\Classes\AppUserModelId\<AUMID>`. Buttons use protocol activation with `file:` URIs, so "Ochish"
//! opens the file in its default app and "Papkada ko'rsatish" opens the folder, with no COM activator to register.

use std::fmt::Write as _;
use std::path::Path;

use crate::i18n::{Lang, t, tf};

/// Our `AppUserModelID`. Also the MSIX application id, so packaged and unpackaged builds share toast history.
pub const AUMID: &str = "Wonmor.Vutric";

/// One "files arrived" notification.
#[derive(Debug, Clone)]
pub struct Received<'a> {
    pub device: &'a str,
    pub files: &'a [std::path::PathBuf],
    pub folder: &'a Path,
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// A `file:///` URI for an absolute path, percent-encoding everything outside the unreserved set.
#[must_use]
pub fn file_uri(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file://");
    if !s.starts_with('/') {
        out.push('/');
    }
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => {
                out.push(b as char);
            }
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

/// The toast XML for `r`. A single image also shows as the toast's picture.
#[must_use]
pub fn received_xml(lang: Lang, r: &Received<'_>) -> String {
    let title = match r.files {
        [one] => tf(
            lang,
            "toast.received.one",
            &[(
                "name",
                &one.file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
            )],
        ),
        many => tf(
            lang,
            "toast.received.many",
            &[("count", &many.len().to_string())],
        ),
    };
    let body = tf(lang, "toast.from", &[("device", r.device)]);
    let folder = file_uri(r.folder);
    let open = r
        .files
        .first()
        .map_or_else(|| folder.clone(), |f| file_uri(f));
    let image = match r.files {
        [one] if is_image(one) => format!(
            r#"<image placement="hero" src="{}"/>"#,
            xml_escape(&file_uri(one))
        ),
        _ => String::new(),
    };
    format!(
        concat!(
            r#"<toast activationType="protocol" launch="{open}">"#,
            r#"<visual><binding template="ToastGeneric"><text>{title}</text><text>{body}</text>{image}</binding></visual>"#,
            r#"<actions>"#,
            r#"<action content="{open_label}" activationType="protocol" arguments="{open}"/>"#,
            r#"<action content="{folder_label}" activationType="protocol" arguments="{folder}"/>"#,
            r#"</actions></toast>"#
        ),
        open = xml_escape(&open),
        title = xml_escape(&title),
        body = xml_escape(&body),
        image = image,
        open_label = xml_escape(t(lang, "toast.open")),
        folder_label = xml_escape(t(lang, "toast.show_in_folder")),
        folder = xml_escape(&folder),
    )
}

fn is_image(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp"
        )
    })
}

#[cfg(windows)]
mod imp {
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
    use windows::core::HSTRING;

    use crate::winreg;

    /// Registers the AUMID's display name and icon (idempotent, per user, no admin).
    pub fn register(
        display_name: &str,
        icon: Option<&std::path::Path>,
    ) -> windows::core::Result<()> {
        let key = format!(r"Software\Classes\AppUserModelId\{}", super::AUMID);
        winreg::set_string(&key, "DisplayName", display_name)?;
        if let Some(icon) = icon {
            winreg::set_string(&key, "IconUri", &icon.to_string_lossy())?;
        }
        Ok(())
    }

    pub fn show(xml: &str) -> windows::core::Result<()> {
        let doc = XmlDocument::new()?;
        doc.LoadXml(&HSTRING::from(xml))?;
        let toast = ToastNotification::CreateToastNotification(&doc)?;
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(super::AUMID))?
            .Show(&toast)
    }
}

#[cfg(windows)]
pub use imp::{register, show};

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn uris_are_encoded() {
        assert_eq!(
            file_uri(Path::new(r"C:\Users\Ali Vali\Downloads\Vutric\a&b #1.jpg")),
            "file:///C:/Users/Ali%20Vali/Downloads/Vutric/a%26b%20%231.jpg"
        );
        assert_eq!(file_uri(Path::new("/home/u/x y")), "file:///home/u/x%20y");
        assert_eq!(
            file_uri(Path::new("C:\\O\u{2bb}zbek")),
            "file:///C:/O%CA%BBzbek"
        );
    }

    #[cfg(windows)]
    #[test]
    fn one_photo_has_a_picture_and_two_buttons() {
        let files = [PathBuf::from(r"C:\D\photo.JPG")];
        let xml = received_xml(
            Lang::Uz,
            &Received {
                device: "Ali's <Pixel>",
                files: &files,
                folder: Path::new(r"C:\D"),
            },
        );
        assert!(xml.contains("<text>photo.JPG keldi</text>"));
        assert!(xml.contains("Ali&apos;s &lt;Pixel&gt; dan"));
        assert!(xml.contains(r#"<image placement="hero" src="file:///C:/D/photo.JPG"/>"#));
        assert!(xml.contains(r#"content="Ochish""#));
        assert!(xml.contains(r#"content="Papkada ko&apos;rsatish" activationType="protocol" arguments="file:///C:/D""#));
        assert_eq!(xml.matches("<action ").count(), 2);
    }

    #[test]
    fn many_files_show_a_count_and_no_picture() {
        let files = [PathBuf::from("/d/a.pdf"), PathBuf::from("/d/b.mp4")];
        let xml = received_xml(
            Lang::En,
            &Received {
                device: "Phone",
                files: &files,
                folder: Path::new("/d"),
            },
        );
        assert!(xml.contains("<text>2 files arrived</text>"));
        assert!(!xml.contains("<image"));
    }

    #[test]
    fn control_characters_are_dropped() {
        assert_eq!(xml_escape("a\u{0}b\u{1b}[2Jc"), "ab[2Jc");
    }
}
