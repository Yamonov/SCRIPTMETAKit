use std::{
    fs::File,
    io::{self, Read},
    path::Path,
    time::Duration,
};

use serde::{Deserialize, Serialize};

#[cfg(target_os = "macos")]
use std::{
    io::Write,
    process::{Command, ExitStatus, Stdio},
    thread,
    time::Instant,
};

#[cfg(target_os = "macos")]
use sha2::{Digest, Sha256};

#[cfg(target_os = "macos")]
use crate::core::decode_script_text;
use crate::core::{ScriptRuntimeKind, parse_script_metadata};

#[cfg(target_os = "macos")]
const DEFAULT_DECOMPILE_TIMEOUT: Duration = Duration::from_millis(3_000);
#[cfg(target_os = "macos")]
const DEFAULT_COMPILE_TIMEOUT: Duration = Duration::from_millis(5_000);
#[cfg(target_os = "macos")]
const COMPILED_OSA_LANGUAGE_HINT_BYTES: usize = 64 * 1024;
const SCRIPT_META_BEGIN: &[u8] = b"SCRIPTMETA-BEGIN";
const SCRIPT_META_END: &[u8] = b"SCRIPTMETA-END";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct CompiledOsaSource {
    pub source: String,
    pub language_hint: Option<ScriptRuntimeKind>,
    pub source_fingerprint: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CompiledOsaMetadataSnippet {
    pub text: String,
    pub language_hint: Option<ScriptRuntimeKind>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CompiledOsaErrorKind {
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    SourceUnavailable,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    NotOsaScript,
    PermissionDenied,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Timeout,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    ToolUnavailable,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    ProcessFailed,
    Io,
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    UnsupportedPlatform,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CompiledOsaError {
    pub kind: CompiledOsaErrorKind,
    pub message: String,
}

impl CompiledOsaError {
    fn new(kind: CompiledOsaErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

#[must_use]
pub(crate) fn is_compiled_osa_path(path: &Path) -> bool {
    super::is_compiled_osa_path(path)
}

pub(crate) fn extract_compiled_osa_metadata_fast(
    path: &Path,
    max_bytes: usize,
    file_size: u64,
) -> Result<Option<CompiledOsaMetadataSnippet>, CompiledOsaError> {
    if max_bytes == 0 {
        return Ok(None);
    }

    let mut file = File::open(path).map_err(|error| compiled_osa_io_error(path, error))?;
    let capacity = usize::try_from(file_size)
        .unwrap_or(max_bytes)
        .min(max_bytes);
    let mut buffer = Vec::with_capacity(capacity);
    Read::by_ref(&mut file)
        .take(max_bytes as u64)
        .read_to_end(&mut buffer)
        .map_err(|error| compiled_osa_io_error(path, error))?;

    Ok(extract_metadata_snippet_from_bytes(&buffer))
}

fn extract_metadata_snippet_from_bytes(bytes: &[u8]) -> Option<CompiledOsaMetadataSnippet> {
    let language_hint = language_hint_from_compiled_bytes(bytes);
    let text = metadata_text_from_bytes(bytes)?;
    parse_script_metadata(&text).ok()?;
    Some(CompiledOsaMetadataSnippet {
        text,
        language_hint,
    })
}

fn metadata_text_from_bytes(bytes: &[u8]) -> Option<String> {
    utf16_metadata_block(bytes, Utf16Endian::Little)
        .or_else(|| utf16_metadata_block(bytes, Utf16Endian::Big))
        .or_else(|| {
            let block = ascii_metadata_block(bytes)?;
            std::str::from_utf8(block).ok().map(str::to_string)
        })
}

fn ascii_metadata_block(bytes: &[u8]) -> Option<&[u8]> {
    let begin = find_bytes(bytes, SCRIPT_META_BEGIN)?;
    let after_begin = begin + SCRIPT_META_BEGIN.len();
    let end = after_begin + find_bytes(&bytes[after_begin..], SCRIPT_META_END)?;
    let after_end = end + SCRIPT_META_END.len();
    let block = &bytes[begin..after_end];
    block.is_ascii().then_some(block)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Utf16Endian {
    Little,
    Big,
}

fn utf16_metadata_block(bytes: &[u8], endian: Utf16Endian) -> Option<String> {
    let begin_marker = utf16_marker(SCRIPT_META_BEGIN, endian);
    let end_marker = utf16_marker(SCRIPT_META_END, endian);
    let begin = find_bytes(bytes, &begin_marker)?;
    let after_begin = begin + begin_marker.len();
    let end = after_begin + find_bytes(&bytes[after_begin..], &end_marker)?;
    let after_end = end + end_marker.len();
    decode_utf16_without_bom(&bytes[begin..after_end], endian)
}

fn utf16_marker(marker: &[u8], endian: Utf16Endian) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(marker.len() * 2);
    for byte in marker {
        match endian {
            Utf16Endian::Little => {
                encoded.push(*byte);
                encoded.push(0);
            }
            Utf16Endian::Big => {
                encoded.push(0);
                encoded.push(*byte);
            }
        }
    }
    encoded
}

fn decode_utf16_without_bom(bytes: &[u8], endian: Utf16Endian) -> Option<String> {
    let chunks = bytes.chunks_exact(2);
    if !chunks.remainder().is_empty() {
        return None;
    }
    let units = chunks
        .map(|chunk| match endian {
            Utf16Endian::Little => u16::from_le_bytes([chunk[0], chunk[1]]),
            Utf16Endian::Big => u16::from_be_bytes([chunk[0], chunk[1]]),
        })
        .collect::<Vec<_>>();
    String::from_utf16(&units).ok()
}

#[cfg(target_os = "macos")]
pub(crate) fn decompile_compiled_osa_source(
    path: &Path,
    timeout: Option<Duration>,
) -> Result<CompiledOsaSource, CompiledOsaError> {
    let language_hint = read_compiled_osa_language_hint(path)?;
    let output = run_command_with_timeout(
        CommandSpec {
            program: "osadecompile",
            args: vec![path.as_os_str().to_owned()],
            stdin: None,
        },
        timeout.unwrap_or(DEFAULT_DECOMPILE_TIMEOUT),
    )?;
    if !output.status.success() {
        return Err(classify_decompile_failure(path, &output));
    }

    let source = decode_script_text(&output.stdout);
    let language_hint = language_hint.or_else(|| language_hint_from_source(&source));
    let source_fingerprint = fingerprint(source.as_bytes());
    Ok(CompiledOsaSource {
        source,
        language_hint,
        source_fingerprint,
    })
}

#[cfg(target_os = "macos")]
fn read_compiled_osa_language_hint(
    path: &Path,
) -> Result<Option<ScriptRuntimeKind>, CompiledOsaError> {
    let mut file = File::open(path).map_err(|error| compiled_osa_io_error(path, error))?;
    let mut buffer = Vec::with_capacity(COMPILED_OSA_LANGUAGE_HINT_BYTES);
    Read::by_ref(&mut file)
        .take(COMPILED_OSA_LANGUAGE_HINT_BYTES as u64)
        .read_to_end(&mut buffer)
        .map_err(|error| compiled_osa_io_error(path, error))?;
    Ok(language_hint_from_compiled_bytes(&buffer))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn decompile_compiled_osa_source(
    path: &Path,
    _timeout: Option<Duration>,
) -> Result<CompiledOsaSource, CompiledOsaError> {
    Err(CompiledOsaError::new(
        CompiledOsaErrorKind::UnsupportedPlatform,
        format!(
            "{} is a compiled OSA script, which can only be decompiled on macOS",
            path.display()
        ),
    ))
}

#[cfg(target_os = "macos")]
pub(crate) fn compile_osa_source_to_path(
    source: &str,
    language_hint: Option<ScriptRuntimeKind>,
    output_path: &Path,
    timeout: Option<Duration>,
) -> Result<(), CompiledOsaError> {
    let mut args = Vec::new();
    if language_hint == Some(ScriptRuntimeKind::JavaScriptForAutomation) {
        args.push(std::ffi::OsString::from("-l"));
        args.push(std::ffi::OsString::from("JavaScript"));
    } else if language_hint == Some(ScriptRuntimeKind::AppleScript) {
        args.push(std::ffi::OsString::from("-l"));
        args.push(std::ffi::OsString::from("AppleScript"));
    }
    args.push(std::ffi::OsString::from("-o"));
    args.push(output_path.as_os_str().to_owned());

    let output = run_command_with_timeout(
        CommandSpec {
            program: "osacompile",
            args,
            stdin: Some(source.as_bytes().to_vec()),
        },
        timeout.unwrap_or(DEFAULT_COMPILE_TIMEOUT),
    )?;
    if output.status.success() {
        return Ok(());
    }
    Err(classify_compile_failure(output_path, &output))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn compile_osa_source_to_path(
    _source: &str,
    _language_hint: Option<ScriptRuntimeKind>,
    output_path: &Path,
    _timeout: Option<Duration>,
) -> Result<(), CompiledOsaError> {
    Err(CompiledOsaError::new(
        CompiledOsaErrorKind::UnsupportedPlatform,
        format!(
            "{} is a compiled OSA script, which can only be compiled on macOS",
            output_path.display()
        ),
    ))
}

#[must_use]
pub(crate) fn language_hint_from_source(source: &str) -> Option<ScriptRuntimeKind> {
    let trimmed = source.trim_start();
    if trimmed.starts_with("/*")
        || trimmed.starts_with("//")
        || trimmed.starts_with("ObjC.")
        || trimmed.starts_with("ObjC.import")
        || trimmed.starts_with("Application(")
        || trimmed.starts_with("function ")
        || trimmed.starts_with("(()")
        || trimmed.contains("Application(\"")
        || trimmed.contains("Application('")
    {
        Some(ScriptRuntimeKind::JavaScriptForAutomation)
    } else {
        Some(ScriptRuntimeKind::AppleScript)
    }
}

#[must_use]
pub(crate) fn language_hint_from_compiled_bytes(bytes: &[u8]) -> Option<ScriptRuntimeKind> {
    if contains_bytes(bytes, b"jscr") || contains_bytes(bytes, b"JsOsa") {
        Some(ScriptRuntimeKind::JavaScriptForAutomation)
    } else if contains_bytes(bytes, b"ascr") {
        Some(ScriptRuntimeKind::AppleScript)
    } else {
        None
    }
}

fn compiled_osa_io_error(path: &Path, error: io::Error) -> CompiledOsaError {
    let kind = match error.kind() {
        io::ErrorKind::PermissionDenied => CompiledOsaErrorKind::PermissionDenied,
        _ => CompiledOsaErrorKind::Io,
    };
    CompiledOsaError::new(kind, format!("{}: {error}", path.display()))
}

#[cfg(target_os = "macos")]
fn classify_decompile_failure(path: &Path, output: &CommandOutput) -> CompiledOsaError {
    let stderr = decode_script_text(&output.stderr);
    let message = if stderr.trim().is_empty() {
        format!("osadecompile failed for {}", path.display())
    } else {
        stderr.trim().to_string()
    };
    let kind = if stderr.contains("errOSASourceNotAvailable") || stderr.contains("-1756") {
        CompiledOsaErrorKind::SourceUnavailable
    } else if stderr.contains("errOSAScriptError") || stderr.contains("-1753") {
        CompiledOsaErrorKind::NotOsaScript
    } else if stderr.to_ascii_lowercase().contains("permission") {
        CompiledOsaErrorKind::PermissionDenied
    } else {
        CompiledOsaErrorKind::ProcessFailed
    };
    CompiledOsaError::new(kind, message)
}

#[cfg(target_os = "macos")]
fn classify_compile_failure(path: &Path, output: &CommandOutput) -> CompiledOsaError {
    let stderr = decode_script_text(&output.stderr);
    let message = if stderr.trim().is_empty() {
        format!("osacompile failed for {}", path.display())
    } else {
        stderr.trim().to_string()
    };
    let kind = if stderr.to_ascii_lowercase().contains("permission") {
        CompiledOsaErrorKind::PermissionDenied
    } else {
        CompiledOsaErrorKind::ProcessFailed
    };
    CompiledOsaError::new(kind, message)
}

#[cfg(target_os = "macos")]
struct CommandSpec {
    program: &'static str,
    args: Vec<std::ffi::OsString>,
    stdin: Option<Vec<u8>>,
}

#[cfg(target_os = "macos")]
struct CommandOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

#[cfg(target_os = "macos")]
fn run_command_with_timeout(
    spec: CommandSpec,
    timeout: Duration,
) -> Result<CommandOutput, CompiledOsaError> {
    let mut command = Command::new(spec.program);
    command
        .args(&spec.args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if spec.stdin.is_some() {
        command.stdin(Stdio::piped());
    }

    let mut child = command.spawn().map_err(|error| {
        let kind = if error.kind() == io::ErrorKind::NotFound {
            CompiledOsaErrorKind::ToolUnavailable
        } else {
            CompiledOsaErrorKind::Io
        };
        CompiledOsaError::new(kind, format!("{}: {error}", spec.program))
    })?;

    if let Some(input) = spec.stdin
        && let Some(mut stdin) = child.stdin.take()
    {
        thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
    }

    let stdout = child.stdout.take().map(read_pipe_in_thread);
    let stderr = child.stderr.take().map(read_pipe_in_thread);
    let started = Instant::now();
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| CompiledOsaError::new(CompiledOsaErrorKind::Io, error.to_string()))?
        {
            return Ok(CommandOutput {
                status,
                stdout: stdout.and_then(join_reader).unwrap_or_default(),
                stderr: stderr.and_then(join_reader).unwrap_or_default(),
            });
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CompiledOsaError::new(
                CompiledOsaErrorKind::Timeout,
                format!(
                    "{} timed out after {} ms",
                    spec.program,
                    timeout.as_millis()
                ),
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(target_os = "macos")]
fn read_pipe_in_thread<R>(mut reader: R) -> thread::JoinHandle<Vec<u8>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = reader.read_to_end(&mut buffer);
        buffer
    })
}

#[cfg(target_os = "macos")]
fn join_reader(handle: thread::JoinHandle<Vec<u8>>) -> Option<Vec<u8>> {
    handle.join().ok()
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    find_bytes(haystack, needle).is_some()
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(target_os = "macos")]
fn fingerprint(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{
        extract_metadata_snippet_from_bytes, language_hint_from_compiled_bytes,
        language_hint_from_source,
    };
    use crate::core::ScriptRuntimeKind;

    #[test]
    fn detects_language_hints_from_source() {
        assert_eq!(
            language_hint_from_source("Application(\"Finder\").name();"),
            Some(ScriptRuntimeKind::JavaScriptForAutomation)
        );
        assert_eq!(
            language_hint_from_source("display dialog \"hello\""),
            Some(ScriptRuntimeKind::AppleScript)
        );
    }

    #[test]
    fn detects_language_hints_from_compiled_markers() {
        assert_eq!(
            language_hint_from_compiled_bytes(b"prefix jscr suffix"),
            Some(ScriptRuntimeKind::JavaScriptForAutomation)
        );
        assert_eq!(
            language_hint_from_compiled_bytes(b"prefix ascr suffix"),
            Some(ScriptRuntimeKind::AppleScript)
        );
    }

    #[test]
    fn extracts_ascii_metadata_without_decompile() {
        let snippet = extract_metadata_snippet_from_bytes(
            b"binary ascr prefix SCRIPTMETA-BEGIN\nScript-ID=com.example.test\nVersion=1.2.3\nSCRIPTMETA-END binary suffix",
        )
        .expect("snippet");

        assert_eq!(snippet.language_hint, Some(ScriptRuntimeKind::AppleScript));
        assert!(snippet.text.contains("Script-ID=com.example.test"));
        assert!(snippet.text.contains("Version=1.2.3"));
    }

    #[test]
    fn rejects_non_ascii_metadata_fast_path() {
        assert!(
            extract_metadata_snippet_from_bytes(
                b"SCRIPTMETA-BEGIN\nScript-ID=com.example.test\nName=\xe6\x97\xa5\xe6\x9c\xac\xe8\xaa\x9e\nSCRIPTMETA-END",
            )
            .is_none()
        );
    }

    #[test]
    fn rejects_incomplete_metadata_fast_path() {
        assert!(
            extract_metadata_snippet_from_bytes(b"SCRIPTMETA-BEGIN\nScript-ID=com.example.test\n")
                .is_none()
        );
    }

    #[test]
    fn extracts_utf16_metadata_before_ascii_fallback() {
        let mut bytes = b"binary ascr prefix ".to_vec();
        for unit in "SCRIPTMETA-BEGIN\nScript-ID=com.example.test\nName=日本語\nSCRIPTMETA-END"
            .encode_utf16()
        {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes.extend_from_slice(
            b" SCRIPTMETA-BEGIN\nScript-ID=com.example.test\nName=????\nSCRIPTMETA-END",
        );

        let snippet = extract_metadata_snippet_from_bytes(&bytes).expect("snippet");
        assert!(snippet.text.contains("Name=日本語"));
    }
}
