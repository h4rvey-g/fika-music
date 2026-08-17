use std::collections::BTreeMap;
use std::time::{Duration, Instant};

const PREFERENCE_TTL: Duration = Duration::from_secs(10 * 60);

#[derive(Debug)]
struct ChannelPreference {
    audio_source_id: String,
    last_success_at: Instant,
}

#[derive(Debug, Default)]
pub(crate) struct ChannelAudioSourcePreferences {
    entries: BTreeMap<String, ChannelPreference>,
}

impl ChannelAudioSourcePreferences {
    pub(crate) fn snapshot(&mut self, now: Instant) -> BTreeMap<String, String> {
        self.prune(now);
        self.entries
            .iter()
            .map(|(channel_id, preference)| {
                (channel_id.clone(), preference.audio_source_id.clone())
            })
            .collect()
    }

    pub(crate) fn report_success(&mut self, channel_id: &str, audio_source_id: &str, now: Instant) {
        self.prune(now);
        if self
            .entries
            .get(channel_id)
            .is_some_and(|preference| preference.last_success_at > now)
        {
            return;
        }
        self.entries.insert(
            channel_id.to_owned(),
            ChannelPreference {
                audio_source_id: audio_source_id.to_owned(),
                last_success_at: now,
            },
        );
    }

    fn prune(&mut self, now: Instant) {
        self.entries.retain(|_, preference| {
            now.saturating_duration_since(preference.last_success_at) <= PREFERENCE_TTL
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_returns_the_recent_success_for_a_channel() {
        let now = Instant::now();
        let mut preferences = ChannelAudioSourcePreferences::default();
        preferences.report_success("netease", "source-a", now);

        assert_eq!(
            preferences.snapshot(now).get("netease").map(String::as_str),
            Some("source-a")
        );
    }

    #[test]
    fn later_success_replaces_the_channel_preference() {
        let now = Instant::now();
        let mut preferences = ChannelAudioSourcePreferences::default();
        preferences.report_success("netease", "source-a", now);
        preferences.report_success("netease", "source-b", now + Duration::from_millis(1));

        assert_eq!(
            preferences
                .snapshot(now + Duration::from_millis(1))
                .get("netease")
                .map(String::as_str),
            Some("source-b")
        );
    }

    #[test]
    fn delayed_older_report_does_not_replace_the_latest_success() {
        let now = Instant::now();
        let mut preferences = ChannelAudioSourcePreferences::default();
        preferences.report_success("netease", "source-b", now + Duration::from_millis(2));
        preferences.report_success("netease", "source-a", now + Duration::from_millis(1));

        assert_eq!(
            preferences
                .snapshot(now + Duration::from_millis(2))
                .get("netease")
                .map(String::as_str),
            Some("source-b")
        );
    }

    #[test]
    fn snapshot_expires_a_stale_channel_preference() {
        let now = Instant::now();
        let mut preferences = ChannelAudioSourcePreferences::default();
        preferences.report_success("netease", "source-a", now);

        assert!(preferences
            .snapshot(now + PREFERENCE_TTL + Duration::from_millis(1))
            .is_empty());
    }
}
