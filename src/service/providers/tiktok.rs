use reqwest::Url;

use crate::module::processing::model::PROVIDER_TIKTOK;

pub const PROVIDER: &str = PROVIDER_TIKTOK;

pub fn matches(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };

    let host = host.to_ascii_lowercase();

    host == "tiktok.com"
        || host.ends_with(".tiktok.com")
        || host == "vm.tiktok.com"
        || host == "vt.tiktok.com"
}
