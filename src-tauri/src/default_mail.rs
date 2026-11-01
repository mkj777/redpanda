//! redpanda as the default mail app: opens `mailto:` links.
//!
//! Since Windows 10 an app cannot make itself the default. It registers as a handler for
//! `mailto:`, and the user picks it in the Windows settings, which redpanda opens for them.

use serde::Serialize;

/// The ProgId Windows stores as the user's choice when redpanda is the default.
#[cfg(windows)]
const PROG_ID: &str = "redpanda.mailto";
/// The name under RegisteredApplications, also used to open redpanda's page in the settings.
#[cfg(windows)]
const APP_NAME: &str = "redpanda";
#[cfg(windows)]
const CAPABILITIES: &str = r"Software\Clients\Mail\redpanda\Capabilities";

/// Only installed builds register, a debug build would point Windows at the dev binary.
pub fn supported() -> bool {
    cfg!(windows) && !cfg!(debug_assertions)
}

/// Writes the registry entries that let Windows offer redpanda for `mailto:`. Runs at every
/// start, so the path stays right after an update or a move.
#[cfg(windows)]
pub fn register() -> std::io::Result<()> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let exe = std::env::current_exe()?;
    let exe = exe.to_string_lossy();
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    let (prog, _) = hkcu.create_subkey(format!(r"Software\Classes\{PROG_ID}"))?;
    prog.set_value("", &"redpanda mail link")?;
    let (icon, _) = prog.create_subkey("DefaultIcon")?;
    icon.set_value("", &format!("\"{exe}\",0"))?;
    let (cmd, _) = prog.create_subkey(r"shell\open\command")?;
    cmd.set_value("", &format!("\"{exe}\" \"%1\""))?;

    let (caps, _) = hkcu.create_subkey(CAPABILITIES)?;
    caps.set_value("ApplicationName", &APP_NAME)?;
    caps.set_value(
        "ApplicationDescription",
        &"Mail client for Gmail, Outlook and IMAP",
    )?;
    let (urls, _) = caps.create_subkey("URLAssociations")?;
    urls.set_value("mailto", &PROG_ID)?;

    let (apps, _) = hkcu.create_subkey(r"Software\RegisteredApplications")?;
    apps.set_value(APP_NAME, &CAPABILITIES)?;
    Ok(())
}

#[cfg(not(windows))]
pub fn register() -> std::io::Result<()> {
    Ok(())
}

/// Whether the user's choice for `mailto:` is redpanda.
#[cfg(windows)]
pub fn is_default() -> bool {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let base = r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\mailto";
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let read = |path: String| {
        hkcu.open_subkey(path)
            .and_then(|k| k.get_value::<String, _>("ProgId"))
            .ok()
    };
    // Newer Windows 11 builds keep the choice under UserChoiceLatest, older ones only under
    // UserChoice.
    read(format!(r"{base}\UserChoiceLatest\ProgId"))
        .or_else(|| read(format!(r"{base}\UserChoice")))
        .is_some_and(|p| p.eq_ignore_ascii_case(PROG_ID))
}

#[cfg(not(windows))]
pub fn is_default() -> bool {
    false
}

/// The Windows settings page where the user picks redpanda.
#[cfg(windows)]
pub fn settings_uri() -> String {
    format!("ms-settings:defaultapps?registeredAppUser={APP_NAME}")
}

#[cfg(not(windows))]
pub fn settings_uri() -> String {
    String::new()
}

/// A `mailto:` link, split into the fields of a new draft (RFC 6068).
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mailto {
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body: String,
}

fn decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .into_owned()
}

fn addresses(s: &str) -> impl Iterator<Item = String> + '_ {
    s.split(',')
        .map(|a| decode(a).trim().to_string())
        .filter(|a| a.contains('@'))
}

pub fn parse(uri: &str) -> Option<Mailto> {
    if !uri.get(..7)?.eq_ignore_ascii_case("mailto:") {
        return None;
    }
    let rest = &uri[7..];
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let mut m = Mailto {
        to: addresses(path).collect(),
        ..Mailto::default()
    };
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        match key.to_ascii_lowercase().as_str() {
            "to" => m.to.extend(addresses(value)),
            "cc" => m.cc.extend(addresses(value)),
            "bcc" => m.bcc.extend(addresses(value)),
            "subject" => m.subject = decode(value),
            "body" => m.body = decode(value).replace("\r\n", "\n"),
            _ => {}
        }
    }
    Some(m)
}

/// The `mailto:` link among the arguments Windows started redpanda with.
pub fn from_args<I: IntoIterator<Item = S>, S: AsRef<str>>(args: I) -> Option<Mailto> {
    args.into_iter().find_map(|a| parse(a.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_fields() {
        let m = parse("MAILTO:a@x.example,b%40x.example?cc=c@x.example&BCC=d@x.example&subject=Hi%20there&body=Line%201%0D%0ALine+2").unwrap();
        assert_eq!(m.to, ["a@x.example", "b@x.example"]);
        assert_eq!(m.cc, ["c@x.example"]);
        assert_eq!(m.bcc, ["d@x.example"]);
        assert_eq!(m.subject, "Hi there");
        // A plus is a plus in mailto, not a space.
        assert_eq!(m.body, "Line 1\nLine+2");
    }

    #[test]
    fn handles_bare_and_empty_links() {
        assert_eq!(parse("mailto:a@x.example").unwrap().to, ["a@x.example"]);
        let m = parse("mailto:?to=a@x.example&subject=x").unwrap();
        assert_eq!(m.to, ["a@x.example"]);
        assert!(parse("mailto:").unwrap().to.is_empty());
        assert!(parse("https://x.example").is_none());
    }

    #[test]
    fn finds_the_link_among_arguments() {
        let m = from_args(["redpanda.exe", "--autostart", "mailto:a@x.example"]).unwrap();
        assert_eq!(m.to, ["a@x.example"]);
        assert!(from_args(["redpanda.exe"]).is_none());
    }
}
