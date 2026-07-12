use scriptmetakit::ScriptMetaKitConfig;

const MAX_WATCHER_PENDING_PATHS: usize = 1_048_576;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SmkOperationalPolicy {
    pub max_concurrent_meta_url_checks: usize,
    pub retry_attempts: usize,
    pub retry_initial_delay_millis: u64,
    pub retry_backoff_multiplier: u32,
    pub max_retry_delay_millis: u64,
    pub request_timeout_millis: u64,
    pub resource_timeout_millis: u64,
    pub watcher_debounce_delay_millis: u64,
    pub watcher_max_delivery_delay_millis: u64,
    pub watcher_max_pending_paths: usize,
}

pub(crate) fn apply_operational_policy(
    config: &mut ScriptMetaKitConfig,
    policy: &SmkOperationalPolicy,
) -> Result<(), &'static str> {
    if policy.max_concurrent_meta_url_checks == 0
        || policy.max_concurrent_meta_url_checks > 64
        || policy.retry_attempts > 10
        || !(1..=16).contains(&policy.retry_backoff_multiplier)
        || policy.max_retry_delay_millis < policy.retry_initial_delay_millis
        || policy.request_timeout_millis == 0
        || policy.resource_timeout_millis == 0
        || policy.watcher_max_pending_paths == 0
        || policy.watcher_max_pending_paths > MAX_WATCHER_PENDING_PATHS
    {
        return Err("operational policy contains an invalid limit");
    }
    config.update_check.max_concurrent_meta_url_checks = policy.max_concurrent_meta_url_checks;
    config.update_check.retry_attempts = policy.retry_attempts;
    config.update_check.retry_initial_delay_millis = policy.retry_initial_delay_millis;
    config.update_check.retry_backoff_multiplier = policy.retry_backoff_multiplier;
    config.update_check.max_retry_delay_millis = policy.max_retry_delay_millis;
    config.update_check.request_timeout_millis = Some(policy.request_timeout_millis);
    config.update_check.resource_timeout_millis = Some(policy.resource_timeout_millis);
    config.watcher.debounce_delay_millis = policy.watcher_debounce_delay_millis;
    config.watcher.max_delivery_delay_millis = policy.watcher_max_delivery_delay_millis;
    config.watcher.max_pending_paths = policy.watcher_max_pending_paths;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{MAX_WATCHER_PENDING_PATHS, SmkOperationalPolicy, apply_operational_policy};

    #[test]
    fn rejects_an_excessive_watcher_queue_capacity() {
        let mut config = scriptmetakit::ScriptMetaKitConfig::default();
        let policy = SmkOperationalPolicy {
            max_concurrent_meta_url_checks: 6,
            retry_attempts: 2,
            retry_initial_delay_millis: 500,
            retry_backoff_multiplier: 3,
            max_retry_delay_millis: 30_000,
            request_timeout_millis: 15_000,
            resource_timeout_millis: 15_000,
            watcher_debounce_delay_millis: 500,
            watcher_max_delivery_delay_millis: 2_000,
            watcher_max_pending_paths: MAX_WATCHER_PENDING_PATHS + 1,
        };

        assert!(apply_operational_policy(&mut config, &policy).is_err());
    }
}
