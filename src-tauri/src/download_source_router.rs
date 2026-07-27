use crate::audio_source_system::AudioSourceRecord;
use crate::online_music::{AudioSourceSelectionMode, OnlineTrackCandidate};
use crate::source_runtime::{SourceAction, SourceQuality};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

const HEALTH_TTL: Duration = Duration::from_secs(30 * 60);
const RECENT_SUCCESS: Duration = Duration::from_secs(10 * 60);
const BASE_EJECTION: Duration = Duration::from_secs(30);
const MAX_EJECTION: Duration = Duration::from_secs(5 * 60);
const UNKNOWN_ROUTE_SCORE: f64 = 2_500.0;
const EWMA_ALPHA: f64 = 0.25;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct DownloadAttemptKey {
    pub audio_source_id: String,
    pub channel_id: String,
    pub quality: SourceQuality,
}

impl DownloadAttemptKey {
    pub(crate) fn new(
        audio_source_id: impl Into<String>,
        channel_id: impl Into<String>,
        quality: SourceQuality,
    ) -> Self {
        Self {
            audio_source_id: audio_source_id.into(),
            channel_id: channel_id.into(),
            quality,
        }
    }
}

#[derive(Debug, Clone)]
struct AttemptHealth {
    successes: u32,
    failures: u32,
    consecutive_failures: u32,
    ewma_latency_ms: Option<f64>,
    last_success_at: Option<Instant>,
    last_observed_at: Instant,
    ejected_until: Option<Instant>,
}

impl AttemptHealth {
    fn new(now: Instant) -> Self {
        Self {
            successes: 0,
            failures: 0,
            consecutive_failures: 0,
            ewma_latency_ms: None,
            last_success_at: None,
            last_observed_at: now,
            ejected_until: None,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct DownloadSourceRouter {
    health: BTreeMap<DownloadAttemptKey, AttemptHealth>,
}

pub(crate) struct DownloadSourceOrder<'a> {
    pub candidates: &'a [OnlineTrackCandidate],
    pub qualities: &'a [SourceQuality],
    pub mode: AudioSourceSelectionMode,
    pub configured_priority: &'a [String],
    pub selected_audio_source_id: Option<&'a str>,
    pub now: Instant,
}

impl DownloadSourceRouter {
    pub(crate) fn order_sources(
        &mut self,
        records: Vec<AudioSourceRecord>,
        order: DownloadSourceOrder<'_>,
    ) -> Vec<AudioSourceRecord> {
        self.prune(order.now);
        let mut compatible = records
            .into_iter()
            .filter(is_enabled_download_source)
            .filter(|record| !source_routes(record, order.candidates, order.qualities).is_empty())
            .collect::<Vec<_>>();

        if order.mode == AudioSourceSelectionMode::Manual {
            let mut seen = std::collections::BTreeSet::new();
            let manual_order = order
                .selected_audio_source_id
                .into_iter()
                .chain(order.configured_priority.iter().map(String::as_str))
                .filter(|id| seen.insert(*id))
                .collect::<Vec<_>>();
            compatible.sort_by(|left, right| {
                manual_rank(&left.id, &manual_order)
                    .cmp(&manual_rank(&right.id, &manual_order))
                    .then_with(|| left.name.cmp(&right.name))
            });
            return compatible;
        }

        compatible.sort_by(|left, right| {
            self.source_score(left, order.candidates, order.qualities, order.now)
                .partial_cmp(&self.source_score(
                    right,
                    order.candidates,
                    order.qualities,
                    order.now,
                ))
                .unwrap_or(Ordering::Equal)
                .then_with(|| left.name.cmp(&right.name))
        });
        compatible
    }

    pub(crate) fn available_candidates(
        &self,
        audio_source_id: &str,
        candidates: Vec<OnlineTrackCandidate>,
        quality: SourceQuality,
        mode: AudioSourceSelectionMode,
        now: Instant,
    ) -> Vec<OnlineTrackCandidate> {
        if mode == AudioSourceSelectionMode::Manual {
            return candidates;
        }

        let mut available = candidates
            .iter()
            .filter(|candidate| {
                self.is_attempt_available(
                    &DownloadAttemptKey::new(audio_source_id, &candidate.channel_id, quality),
                    now,
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        if !available.is_empty() {
            return available;
        }

        if let Some(recovery) = candidates.into_iter().min_by_key(|candidate| {
            self.health
                .get(&DownloadAttemptKey::new(
                    audio_source_id,
                    &candidate.channel_id,
                    quality,
                ))
                .and_then(|health| health.ejected_until)
        }) {
            available.push(recovery);
        }
        available
    }

    pub(crate) fn hedge_delay(
        &self,
        source: &AudioSourceRecord,
        candidates: &[OnlineTrackCandidate],
        qualities: &[SourceQuality],
    ) -> Duration {
        let baseline = source_routes(source, candidates, qualities)
            .into_iter()
            .filter_map(|route| {
                self.health
                    .get(&route.attempt)
                    .and_then(|health| health.ewma_latency_ms)
            })
            .fold(None::<f64>, |current, latency| {
                Some(current.map_or(latency, |value| value.min(latency)))
            })
            .unwrap_or(900.0);
        Duration::from_millis((baseline * 0.8).clamp(400.0, 1_200.0).round() as u64)
    }

    pub(crate) fn report_success(
        &mut self,
        attempt: DownloadAttemptKey,
        latency: Duration,
        now: Instant,
    ) {
        self.prune(now);
        let health = self
            .health
            .entry(attempt)
            .or_insert_with(|| AttemptHealth::new(now));
        health.successes = health.successes.saturating_add(1);
        health.consecutive_failures = 0;
        health.last_success_at = Some(now);
        health.last_observed_at = now;
        health.ejected_until = None;
        let sample = latency.as_secs_f64().mul_add(1_000.0, 0.0).min(30_000.0);
        health.ewma_latency_ms = Some(match health.ewma_latency_ms {
            Some(current) => EWMA_ALPHA.mul_add(sample, (1.0 - EWMA_ALPHA) * current),
            None => sample,
        });
    }

    pub(crate) fn report_failure(&mut self, attempt: DownloadAttemptKey, now: Instant) {
        self.prune(now);
        let health = self
            .health
            .entry(attempt)
            .or_insert_with(|| AttemptHealth::new(now));
        health.failures = health.failures.saturating_add(1);
        health.consecutive_failures = health.consecutive_failures.saturating_add(1);
        health.last_observed_at = now;
        if health.consecutive_failures >= 2 {
            let multiplier = 2_u32.pow(health.consecutive_failures.saturating_sub(2).min(4));
            let duration = BASE_EJECTION.saturating_mul(multiplier).min(MAX_EJECTION);
            health.ejected_until = now.checked_add(duration);
        }
    }

    fn source_score(
        &self,
        source: &AudioSourceRecord,
        candidates: &[OnlineTrackCandidate],
        qualities: &[SourceQuality],
        now: Instant,
    ) -> f64 {
        source_routes(source, candidates, qualities)
            .into_iter()
            .map(|route| {
                let bias = route.candidate_index as f64 * 25.0 + route.quality_index as f64 * 75.0;
                self.health
                    .get(&route.attempt)
                    .map_or(UNKNOWN_ROUTE_SCORE, |health| self.health_score(health, now))
                    + bias
            })
            .fold(f64::INFINITY, f64::min)
    }

    fn health_score(&self, health: &AttemptHealth, now: Instant) -> f64 {
        if let Some(ejected_until) = health.ejected_until.filter(|until| *until > now) {
            return 1_000_000.0 + ejected_until.duration_since(now).as_secs_f64() * 1_000.0;
        }
        let observations = health.successes.saturating_add(health.failures);
        let success_rate = f64::from(health.successes.saturating_add(1))
            / f64::from(observations.saturating_add(2));
        let latency = health.ewma_latency_ms.unwrap_or(1_500.0);
        let failure_penalty =
            (1.0 - success_rate) * 1_800.0 + f64::from(health.consecutive_failures) * 1_200.0;
        let success_age = health
            .last_success_at
            .map_or(RECENT_SUCCESS, |last| now.saturating_duration_since(last));
        let recency_ratio = (success_age.as_secs_f64() / RECENT_SUCCESS.as_secs_f64()).min(1.0);
        let recency_boost = 700.0 * (1.0 - recency_ratio);
        latency + failure_penalty - recency_boost
    }

    fn is_attempt_available(&self, attempt: &DownloadAttemptKey, now: Instant) -> bool {
        self.health
            .get(attempt)
            .and_then(|health| health.ejected_until)
            .is_none_or(|until| until <= now)
    }

    fn prune(&mut self, now: Instant) {
        self.health.retain(|_, health| {
            now.saturating_duration_since(health.last_observed_at) <= HEALTH_TTL
        });
    }
}

#[derive(Debug)]
struct SourceRoute {
    attempt: DownloadAttemptKey,
    candidate_index: usize,
    quality_index: usize,
}

fn source_routes(
    source: &AudioSourceRecord,
    candidates: &[OnlineTrackCandidate],
    qualities: &[SourceQuality],
) -> Vec<SourceRoute> {
    candidates
        .iter()
        .enumerate()
        .flat_map(|(candidate_index, candidate)| {
            let source_info = source.sources.iter().find(|info| {
                info.id == candidate.source_id && info.actions.contains(&SourceAction::MusicUrl)
            });
            if let Some(source_info) = source_info {
                qualities
                    .iter()
                    .enumerate()
                    .filter(|(_, quality)| {
                        source_info.qualities.is_empty() || source_info.qualities.contains(quality)
                    })
                    .map(|(quality_index, quality)| SourceRoute {
                        attempt: DownloadAttemptKey::new(
                            &source.id,
                            &candidate.channel_id,
                            *quality,
                        ),
                        candidate_index,
                        quality_index,
                    })
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            }
        })
        .collect()
}

fn is_enabled_download_source(record: &AudioSourceRecord) -> bool {
    record.enabled
        && record.state == crate::audio_source_system::AudioSourceState::Enabled
        && record
            .sources
            .iter()
            .any(|source| source.actions.contains(&SourceAction::MusicUrl))
}

fn manual_rank(id: &str, order: &[&str]) -> usize {
    order
        .iter()
        .position(|configured| *configured == id)
        .unwrap_or(order.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_source_system::AudioSourceState;
    use crate::online_music::{OnlineTrack, OnlineTrackCandidate};
    use crate::source_runtime::{SourceInfo, SourceKind};
    use std::collections::{BTreeMap, BTreeSet};

    fn source(id: &str) -> AudioSourceRecord {
        AudioSourceRecord {
            id: id.to_owned(),
            name: id.to_owned(),
            version: None,
            description: None,
            author: None,
            homepage: None,
            path: format!("/{id}"),
            adapter: None,
            state: AudioSourceState::Enabled,
            enabled: true,
            permissions_reviewed: true,
            declared_capabilities: BTreeSet::new(),
            granted_capabilities: BTreeSet::new(),
            sources: vec![SourceInfo {
                id: "wy".to_owned(),
                name: "NetEase".to_owned(),
                kind: SourceKind::Music,
                actions: vec![SourceAction::MusicUrl],
                qualities: vec![SourceQuality::K320],
            }],
            diagnostics: Vec::new(),
            can_remove: true,
            can_enable: true,
        }
    }

    fn track() -> OnlineTrack {
        OnlineTrack {
            key: "track".to_owned(),
            title: "Track".to_owned(),
            artist: "Artist".to_owned(),
            album: None,
            duration_seconds: None,
            cover_url: None,
            track_number: None,
            disc_number: None,
            candidates: vec![OnlineTrackCandidate {
                channel_id: "plugin::wy".to_owned(),
                plugin_id: "plugin".to_owned(),
                source_id: "wy".to_owned(),
                channel_name: "NetEase".to_owned(),
                id: "1".to_owned(),
                title: "Track".to_owned(),
                artist: "Artist".to_owned(),
                album: None,
                duration_seconds: None,
                cover_url: None,
                track_number: None,
                disc_number: None,
                platform_ids: BTreeMap::new(),
                raw_info: serde_json::json!({}),
                rank: 0,
            }],
        }
    }

    #[test]
    fn automatic_mode_prefers_a_recent_fast_success() {
        let now = Instant::now();
        let mut router = DownloadSourceRouter::default();
        router.report_success(
            DownloadAttemptKey::new("source-b", "plugin::wy", SourceQuality::K320),
            Duration::from_millis(120),
            now,
        );

        let ordered = router.order_sources(
            vec![source("source-a"), source("source-b")],
            DownloadSourceOrder {
                candidates: &track().candidates,
                qualities: &[SourceQuality::K320],
                mode: AudioSourceSelectionMode::Automatic,
                configured_priority: &[],
                selected_audio_source_id: None,
                now,
            },
        );

        assert_eq!(ordered[0].id, "source-b");
    }

    #[test]
    fn automatic_mode_temporarily_ejects_repeated_failures_and_allows_recovery() {
        let now = Instant::now();
        let mut router = DownloadSourceRouter::default();
        let attempt = DownloadAttemptKey::new("source-a", "plugin::wy", SourceQuality::K320);
        router.report_failure(attempt.clone(), now);
        router.report_failure(attempt, now + Duration::from_millis(1));

        let ordered = router.order_sources(
            vec![source("source-a"), source("source-b")],
            DownloadSourceOrder {
                candidates: &track().candidates,
                qualities: &[SourceQuality::K320],
                mode: AudioSourceSelectionMode::Automatic,
                configured_priority: &[],
                selected_audio_source_id: None,
                now: now + Duration::from_secs(1),
            },
        );
        assert_eq!(ordered[0].id, "source-b");

        let recovery = router.available_candidates(
            "source-a",
            track().candidates,
            SourceQuality::K320,
            AudioSourceSelectionMode::Automatic,
            now + Duration::from_secs(1),
        );
        assert_eq!(recovery.len(), 1);
    }

    #[test]
    fn manual_mode_preserves_the_task_source_snapshot() {
        let now = Instant::now();
        let mut router = DownloadSourceRouter::default();
        router.report_success(
            DownloadAttemptKey::new("source-b", "plugin::wy", SourceQuality::K320),
            Duration::from_millis(10),
            now,
        );

        let ordered = router.order_sources(
            vec![source("source-a"), source("source-b")],
            DownloadSourceOrder {
                candidates: &track().candidates,
                qualities: &[SourceQuality::K320],
                mode: AudioSourceSelectionMode::Manual,
                configured_priority: &["source-b".to_owned()],
                selected_audio_source_id: Some("source-a"),
                now,
            },
        );

        assert_eq!(
            ordered
                .iter()
                .map(|record| record.id.as_str())
                .collect::<Vec<_>>(),
            vec!["source-a", "source-b"]
        );
    }

    #[test]
    fn compatibility_respects_declared_qualities_and_empty_means_all() {
        let now = Instant::now();
        let mut router = DownloadSourceRouter::default();
        let ordered = router.order_sources(
            vec![source("source-a")],
            DownloadSourceOrder {
                candidates: &track().candidates,
                qualities: &[SourceQuality::K128],
                mode: AudioSourceSelectionMode::Automatic,
                configured_priority: &[],
                selected_audio_source_id: None,
                now,
            },
        );

        assert!(ordered.is_empty());

        let mut universal = source("source-b");
        universal.sources[0].qualities.clear();
        let ordered = router.order_sources(
            vec![universal],
            DownloadSourceOrder {
                candidates: &track().candidates,
                qualities: &[SourceQuality::K128],
                mode: AudioSourceSelectionMode::Automatic,
                configured_priority: &[],
                selected_audio_source_id: None,
                now,
            },
        );
        assert_eq!(ordered.len(), 1);
    }

    #[test]
    fn hedge_delay_is_bounded_and_uses_observed_latency() {
        let now = Instant::now();
        let mut router = DownloadSourceRouter::default();
        let source = source("source-a");
        let track = track();
        assert_eq!(
            router.hedge_delay(&source, &track.candidates, &[SourceQuality::K320]),
            Duration::from_millis(720)
        );

        router.report_success(
            DownloadAttemptKey::new("source-a", "plugin::wy", SourceQuality::K320),
            Duration::from_secs(2),
            now,
        );
        assert_eq!(
            router.hedge_delay(&source, &track.candidates, &[SourceQuality::K320]),
            Duration::from_millis(1_200)
        );
    }
}
