use crate::core::ScriptMetaKitError;

pub(crate) fn is_retryable_source_error(error: &ScriptMetaKitError) -> bool {
    match error {
        ScriptMetaKitError::Timeout(message) => !message.contains("cancelled"),
        ScriptMetaKitError::Url(message) => {
            message.contains("transient_transport:")
                || http_status_from_error_message(message).is_some_and(|status| {
                    matches!(status, 408 | 425 | 429) || (500..=599).contains(&status)
                })
        }
        ScriptMetaKitError::Io { message, .. } => [
            "NotFound",
            "Interrupted",
            "WouldBlock",
            "TimedOut",
            "ConnectionReset",
            "ConnectionAborted",
            "BrokenPipe",
            "UnexpectedEof",
        ]
        .iter()
        .any(|kind| message.contains(&format!("kind={kind}"))),
        _ => false,
    }
}

pub(crate) fn retry_after_hint_millis(error: &ScriptMetaKitError) -> Option<u64> {
    let ScriptMetaKitError::Url(message) = error else {
        return None;
    };
    let value = message.split("retry_after_millis=").nth(1)?;
    value
        .split(|character: char| !character.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

fn http_status_from_error_message(message: &str) -> Option<u16> {
    let status = message.split("http_status=").nth(1)?;
    status
        .split(|character: char| !character.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

#[cfg(feature = "blocking-http")]
pub(crate) fn parse_retry_after_millis(value: &str) -> Option<u64> {
    if let Ok(seconds) = value.trim().parse::<u64>() {
        return Some(seconds.saturating_mul(1_000));
    }
    let retry_at = httpdate::parse_http_date(value).ok()?;
    let delay = retry_at.duration_since(std::time::SystemTime::now()).ok()?;
    Some(delay.as_millis().min(u128::from(u64::MAX)) as u64)
}
