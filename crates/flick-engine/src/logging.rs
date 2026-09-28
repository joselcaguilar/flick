//! JSON tracing setup with secret redaction.

use std::{fmt, io, path::Path};

use tracing_appender::{non_blocking::WorkerGuard, rolling};
use tracing_subscriber::{EnvFilter, fmt::MakeWriter};

use crate::config::RuntimeConfig;

/// Guard that keeps the non-blocking log writer alive until shutdown.
pub struct LoggingGuard {
    _guard: WorkerGuard,
}

/// Initializes JSON tracing with daily rotating redacted log files.
pub fn init(config: &RuntimeConfig) -> anyhow::Result<LoggingGuard> {
    let log_dir = config.data_dir.join("logs");
    std::fs::create_dir_all(&log_dir)?;
    let appender = rolling::RollingFileAppender::builder()
        .rotation(rolling::Rotation::DAILY)
        .filename_prefix("flick-engine")
        .filename_suffix("log")
        .max_log_files(8)
        .build(&log_dir)?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let writer = RedactingMakeWriter::new(writer);
    let filter = EnvFilter::try_new(&config.bootstrap.engine.log_level)?;

    tracing_subscriber::fmt()
        .json()
        .with_env_filter(filter)
        .with_writer(writer)
        .try_init()
        .map_err(|err| anyhow::anyhow!("failed to initialize tracing: {err}"))?;

    Ok(LoggingGuard { _guard: guard })
}

/// Redacts secrets from a log line or field value.
#[must_use]
pub fn redact(input: &str) -> String {
    let with_rtsp = redact_rtsp_credentials(input);
    let with_bearer = redact_bearer_tokens(&with_rtsp);
    redact_key_values(&with_bearer)
}

/// A [`MakeWriter`] wrapper that redacts formatted tracing bytes before writing.
#[derive(Clone)]
pub struct RedactingMakeWriter<M> {
    inner: M,
}

impl<M> RedactingMakeWriter<M> {
    /// Wraps a tracing writer factory.
    #[must_use]
    pub const fn new(inner: M) -> Self {
        Self { inner }
    }
}

impl<'a, M> MakeWriter<'a> for RedactingMakeWriter<M>
where
    M: MakeWriter<'a>,
{
    type Writer = RedactingWriter<M::Writer>;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingWriter {
            inner: self.inner.make_writer(),
        }
    }
}

/// A writer that redacts each formatted write chunk.
pub struct RedactingWriter<W> {
    inner: W,
}

impl<W> io::Write for RedactingWriter<W>
where
    W: io::Write,
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        let redacted = redact(&text);
        self.inner.write_all(redacted.as_bytes())?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl<W> fmt::Debug for RedactingWriter<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedactingWriter").finish_non_exhaustive()
    }
}

fn redact_rtsp_credentials(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut rest = input;
    loop {
        let rtsp = rest.find("rtsp://");
        let rtsps = rest.find("rtsps://");
        let next = match (rtsp, rtsps) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) | (None, Some(a)) => Some(a),
            (None, None) => None,
        };
        let Some(pos) = next else {
            output.push_str(rest);
            break;
        };
        output.push_str(&rest[..pos]);
        let url_rest = &rest[pos..];
        let end = url_rest
            .find(|ch: char| ch.is_whitespace() || ch == '"' || ch == '\'')
            .unwrap_or(url_rest.len());
        let url = &url_rest[..end];
        if let Some(at_pos) = url.find('@') {
            if url[..at_pos].contains(':') {
                if let Some(scheme_end) = url.find("://") {
                    output.push_str(&url[..scheme_end + 3]);
                    output.push_str("******@");
                    output.push_str(&url[at_pos + 1..]);
                } else {
                    output.push_str(url);
                }
            } else {
                output.push_str(url);
            }
        } else {
            output.push_str(url);
        }
        rest = &url_rest[end..];
    }
    output
}

fn redact_bearer_tokens(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(pos) = rest.to_ascii_lowercase().find("bearer ") {
        output.push_str(&rest[..pos]);
        output.push_str(&rest[pos..pos + 7]);
        output.push_str("******");
        let after = &rest[pos + 7..];
        let token_end = after
            .find(|ch: char| ch.is_whitespace() || ch == '"' || ch == '\'' || ch == ',')
            .unwrap_or(after.len());
        rest = &after[token_end..];
    }
    output.push_str(rest);
    output
}

fn redact_key_values(input: &str) -> String {
    const KEYS: [&str; 5] = [
        "access_token",
        "refresh_token",
        "authorization",
        "license_key",
        "llat",
    ];

    let mut redacted = input.to_owned();
    for key in KEYS {
        redacted = redact_key_value_once(&redacted, key);
    }
    redacted
}

fn redact_key_value_once(input: &str, key: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    let mut search_from = 0;
    while let Some(relative) = lower[search_from..].find(key) {
        let key_start = search_from + relative;
        let key_end = key_start + key.len();
        let mut value_start = key_end;
        while let Some(ch) = input[value_start..].chars().next() {
            if ch == ':' || ch == '=' || ch.is_whitespace() || ch == '"' {
                value_start += ch.len_utf8();
            } else {
                break;
            }
        }
        if value_start == key_end {
            search_from = key_end;
            continue;
        }
        let value_end = input[value_start..]
            .find(|ch: char| {
                ch.is_whitespace() || ch == '"' || ch == '\'' || ch == ',' || ch == '}'
            })
            .map(|offset| value_start + offset)
            .unwrap_or(input.len());
        output.push_str(&input[cursor..value_start]);
        output.push_str("******");
        cursor = value_end;
        search_from = value_end;
    }
    output.push_str(&input[cursor..]);
    output
}

/// Returns true when a path is inside the engine log directory.
#[must_use]
pub fn is_log_path(data_dir: &Path, path: &Path) -> bool {
    path.starts_with(data_dir.join("logs"))
}
