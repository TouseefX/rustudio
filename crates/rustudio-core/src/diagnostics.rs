//! Per-process diagnostic logs for desktop support.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const MAX_BYTES: u64 = 4 * 1024 * 1024;
const RETENTION: Duration = Duration::from_secs(14 * 24 * 60 * 60);

static SESSION: OnceLock<Session> = OnceLock::new();

struct Session {
    path: PathBuf,
    file: Mutex<File>,
}

fn dir() -> PathBuf {
    crate::paths::cache().join("rustudio").join("logs")
}

pub fn path() -> Option<&'static Path> {
    SESSION.get().map(|session| session.path.as_path())
}

pub fn open_folder() -> io::Result<()> {
    let dir = dir();
    fs::create_dir_all(&dir)?;
    let opener = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    if std::env::var_os("RUSTUDIO_NO_OPEN").is_none() {
        std::process::Command::new(opener).arg(dir).spawn()?;
    }
    Ok(())
}

fn create_session() -> io::Result<Session> {
    let dir = dir();
    fs::create_dir_all(&dir)?;
    for entry in fs::read_dir(&dir)?.flatten() {
        let name = entry.file_name();
        if name
            .to_str()
            .is_some_and(|s| s.starts_with("rustudio-") && s.ends_with(".log"))
            && entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|modified| modified.elapsed().ok())
                .is_some_and(|age| age > RETENTION)
        {
            let _ = fs::remove_file(entry.path());
        }
    }

    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S-%3f");
    let path = dir.join(format!("rustudio-{stamp}-{}.log", std::process::id()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(&path)?;
    Ok(Session {
        path,
        file: Mutex::new(file),
    })
}

pub fn init() {
    if SESSION.get().is_some() {
        return;
    }
    match create_session() {
        Ok(session) => {
            if SESSION.set(session).is_ok() {
                if ::log::set_logger(&DiagnosticLogger).is_ok() {
                    ::log::set_max_level(::log::LevelFilter::Info);
                }
            }
        }
        Err(error) => eprintln!("Could not create a diagnostic log: {error}"),
    }
}

struct DiagnosticLogger;

impl ::log::Log for DiagnosticLogger {
    fn enabled(&self, metadata: &::log::Metadata<'_>) -> bool {
        let target = metadata.target();
        metadata.level() <= ::log::Level::Info && target.starts_with("rustudio")
            // The graphics stack's warnings and errors: a lost device or a
            // refused allocation is otherwise only a panic with no cause.
            || metadata.level() <= ::log::Level::Warn
                && ["wgpu", "naga", "egui_wgpu", "eframe"].iter().any(|t| target.starts_with(t))
    }

    fn log(&self, record: &::log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        if let Some(session) = SESSION.get() {
            let text = record.args().to_string();
            if let Ok(mut file) = session.file.lock() {
                write_record(&mut file, record.level(), record.target(), &text);
            }
        }
    }

    fn flush(&self) {}
}

fn sanitize_log_text(text: &str) -> std::borrow::Cow<'_, str> {
    // Omit messages that look like they carry raw session cookies or auth headers.
    let lower = text.to_ascii_lowercase();
    if [
        "roblosecurity",
        "warning:-do-not-share",
        "authorization:",
        "cookie:",
        "x-csrf-token:",
        "bearer ",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
    {
        return std::borrow::Cow::Borrowed("[message omitted because it may contain credentials]");
    }
    let bounded = text.get(..text.floor_char_boundary(8192)).unwrap_or(text);
    if !bounded.contains("://") {
        return std::borrow::Cow::Borrowed(bounded);
    }
    // Keep URLs readable for diagnostics (e.g. rbxassetid://, https://host/path)
    // while stripping any embedded userinfo (user:pass@) and signed query/fragment.
    let mut out = String::with_capacity(bounded.len());
    let mut rest = bounded;
    while let Some(scheme_sep) = rest.find("://") {
        let after_sep = scheme_sep + 3;
        let url_end = rest[after_sep..]
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | ')' | ']' | '}'))
            .map(|i| after_sep + i)
            .unwrap_or(rest.len());
        out.push_str(&rest[..after_sep]);
        let body = &rest[after_sep..url_end];
        let (path_part, has_query) = match body.find(['?', '#']) {
            Some(q) => (&body[..q], body.as_bytes()[q] == b'?'),
            None => (body, false),
        };
        let authority_end = path_part.find('/').unwrap_or(path_part.len());
        let authority = &path_part[..authority_end];
        let clean_body = match authority.rfind('@') {
            Some(at) => {
                out.push_str("[redacted]@");
                &path_part[at + 1..]
            }
            None => path_part,
        };
        out.push_str(clean_body);
        if has_query {
            out.push_str("?[redacted]");
        }
        rest = &rest[url_end..];
    }
    out.push_str(rest);
    std::borrow::Cow::Owned(out)
}

fn write_record(file: &mut File, level: ::log::Level, target: &str, text: &str) {
    let text = sanitize_log_text(text);

    if file
        .stream_position()
        .is_ok_and(|position| position >= MAX_BYTES)
    {
        if file.set_len(0).and_then(|()| file.rewind()).is_err() {
            return;
        }
        let _ = writeln!(file, "Diagnostic log restarted after reaching 4 MiB");
    }
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let _ = writeln!(file, "{now} {level:5} {target} {:?}", text.as_ref());
}

/// The panic hook can run while the logging lock is already held.
pub(crate) fn panic_at(location: &str) {
    if let Some(session) = SESSION.get() {
        if let Ok(mut file) = session.file.try_lock() {
            write_record(
                &mut file,
                ::log::Level::Error,
                "rustudio::crash",
                &format!("Panic at {location}; see crash report"),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::sanitize_log_text;

    #[test]
    fn redacts_secrets_but_keeps_sanitized_urls() {
        assert_eq!(
            sanitize_log_text("Cookie: .ROBLOSECURITY=secret"),
            "[message omitted because it may contain credentials]"
        );
        assert_eq!(
            sanitize_log_text("Authorization: Bearer abc123"),
            "[message omitted because it may contain credentials]"
        );
        assert_eq!(
            sanitize_log_text("failed to load rbxassetid://12345"),
            "failed to load rbxassetid://12345"
        );
        assert_eq!(
            sanitize_log_text("GET https://user:pw@cdn.example.com/asset/1?sig=secret&exp=9 failed"),
            "GET https://[redacted]@cdn.example.com/asset/1?[redacted] failed"
        );
    }
}
