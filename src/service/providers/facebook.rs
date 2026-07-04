use reqwest::Url;

use crate::module::processing::model::PROVIDER_FACEBOOK;

pub const PROVIDER: &str = PROVIDER_FACEBOOK;

pub fn matches(url: &Url) -> bool {
    matches_host(url, &["facebook.com", "fb.watch"])
}

fn matches_host(url: &Url, hosts: &[&str]) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };

    let host = host.to_ascii_lowercase();

    hosts.iter().any(|candidate| {
        host == *candidate
            || host
                .strip_suffix(candidate)
                .is_some_and(|prefix| prefix.ends_with('.'))
    })
}
