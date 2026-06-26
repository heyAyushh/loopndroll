use std::collections::HashSet;

use crate::model::ClientBaseUrlRaceCandidate;

const DEFAULT_FALLBACK_DELAY_MILLISECONDS: u64 = 350;
const NANOSECONDS_PER_MILLISECOND: u64 = 1_000_000;

#[uniffi::export]
pub fn default_base_url_race_fallback_delay_nanoseconds() -> u64 {
    DEFAULT_FALLBACK_DELAY_MILLISECONDS * NANOSECONDS_PER_MILLISECOND
}

#[uniffi::export]
pub fn plan_base_url_race_candidates(
    base_urls: Vec<String>,
    fallback_delay_nanoseconds: u64,
) -> Vec<ClientBaseUrlRaceCandidate> {
    unique_base_urls(base_urls)
        .into_iter()
        .enumerate()
        .map(|(index, base_url)| ClientBaseUrlRaceCandidate {
            base_url,
            delay_nanoseconds: if index == 0 {
                0
            } else {
                fallback_delay_nanoseconds
            },
        })
        .collect()
}

fn unique_base_urls(base_urls: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for base_url in base_urls {
        let normalized = normalized_base_url(&base_url);
        if normalized.is_empty() || !seen.insert(normalized.clone()) {
            continue;
        }
        unique.push(normalized);
    }
    unique
}

fn normalized_base_url(base_url: &str) -> String {
    base_url.trim().trim_end_matches('/').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_fallback_delay_is_realtime_tuned() {
        assert_eq!(
            default_base_url_race_fallback_delay_nanoseconds(),
            350_000_000
        );
    }

    #[test]
    fn race_plan_starts_preferred_url_immediately_and_defers_fallbacks() {
        let candidates = plan_base_url_race_candidates(
            vec![
                "http://192.168.1.26:8765".to_owned(),
                "http://192.168.1.26:8781".to_owned(),
            ],
            125_000_000,
        );

        assert_eq!(
            candidates,
            vec![
                ClientBaseUrlRaceCandidate {
                    base_url: "http://192.168.1.26:8765".to_owned(),
                    delay_nanoseconds: 0,
                },
                ClientBaseUrlRaceCandidate {
                    base_url: "http://192.168.1.26:8781".to_owned(),
                    delay_nanoseconds: 125_000_000,
                },
            ]
        );
    }

    #[test]
    fn race_plan_deduplicates_before_assigning_delays() {
        let candidates = plan_base_url_race_candidates(
            vec![
                " http://192.168.1.26:8765/ ".to_owned(),
                "http://192.168.1.26:8765".to_owned(),
                "http://192.168.1.26:8781".to_owned(),
            ],
            125_000_000,
        );

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].base_url, "http://192.168.1.26:8765");
        assert_eq!(candidates[0].delay_nanoseconds, 0);
        assert_eq!(candidates[1].base_url, "http://192.168.1.26:8781");
        assert_eq!(candidates[1].delay_nanoseconds, 125_000_000);
    }
}
