use serde::{Deserialize, Serialize};
use url::Url;

use crate::TimestampMillis;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DistributionMetadata {
    pub script_id: Option<String>,
    pub latest_version: Option<String>,
    pub latest_url: Option<Url>,
    pub latest_page_url: Option<Url>,
    pub note: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DistributionResolution {
    pub latest_version: Option<String>,
    pub latest_page_url: Option<Url>,
    pub final_page_url: Url,
    #[serde(default)]
    pub latest_url_history: Vec<Url>,
    pub checked_at: TimestampMillis,
    pub is_unresolved: bool,
    pub note: Option<String>,
    pub redirect_count: Option<u32>,
}
