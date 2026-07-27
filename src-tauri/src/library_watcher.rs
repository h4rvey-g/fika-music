use std::collections::BTreeSet;
use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

const QUIET_PERIOD: Duration = Duration::from_millis(700);
const MAX_BATCH_AGE: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub(crate) struct LibraryChangeBatch {
    pub folder: PathBuf,
    pub paths: Vec<PathBuf>,
    pub force_full_rescan: bool,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BatchDisposition {
    Complete,
    Retry,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum LibraryWatcherError {
    #[error(transparent)]
    Notify(#[from] notify::Error),
    #[error("failed to start the library watcher thread: {0}")]
    Thread(#[from] io::Error),
    #[error("the library watcher stopped unexpectedly")]
    Stopped,
}

enum WatcherMessage {
    SetFolder {
        folder: PathBuf,
        reply: mpsc::SyncSender<Result<(), notify::Error>>,
    },
    Event(notify::Result<Event>),
    Shutdown,
}

#[derive(Default)]
struct PendingChanges {
    paths: BTreeSet<PathBuf>,
    errors: Vec<String>,
    force_full_rescan: bool,
    first_event_at: Option<Instant>,
    last_event_at: Option<Instant>,
}

impl PendingChanges {
    fn is_empty(&self) -> bool {
        self.paths.is_empty() && self.errors.is_empty() && !self.force_full_rescan
    }

    fn record_event(&mut self, event: Event, folder: &PathBuf) {
        if matches!(&event.kind, EventKind::Access(_)) {
            return;
        }

        let event_had_no_paths = event.paths.is_empty();
        let force_full_rescan = event.need_rescan()
            || event_had_no_paths
            || matches!(&event.kind, EventKind::Any | EventKind::Other);
        let previous_path_count = self.paths.len();
        for path in event.paths {
            if path.starts_with(folder) {
                self.paths.insert(path);
            }
        }
        if force_full_rescan {
            self.force_full_rescan = true;
        }
        if self.paths.len() > previous_path_count || force_full_rescan {
            self.touch();
        }
    }

    fn record_error(&mut self, error: notify::Error) {
        self.errors.push(error.to_string());
        self.force_full_rescan = true;
        self.touch();
    }

    fn touch(&mut self) {
        let now = Instant::now();
        self.first_event_at.get_or_insert(now);
        self.last_event_at = Some(now);
    }

    fn next_timeout(&self) -> Option<Duration> {
        let first = self.first_event_at?;
        let last = self.last_event_at?;
        let deadline = std::cmp::min(first + MAX_BATCH_AGE, last + QUIET_PERIOD);
        Some(deadline.saturating_duration_since(Instant::now()))
    }

    fn take_batch(&mut self, folder: PathBuf) -> LibraryChangeBatch {
        LibraryChangeBatch {
            folder,
            paths: std::mem::take(&mut self.paths).into_iter().collect(),
            force_full_rescan: std::mem::take(&mut self.force_full_rescan),
            errors: std::mem::take(&mut self.errors),
        }
    }

    fn retry(&mut self, batch: LibraryChangeBatch) {
        self.paths.extend(batch.paths);
        self.errors.extend(batch.errors);
        self.force_full_rescan |= batch.force_full_rescan;
        let now = Instant::now();
        self.first_event_at = Some(now);
        self.last_event_at = Some(now);
    }

    fn clear(&mut self) {
        *self = Self::default();
    }
}

pub(crate) struct LibraryWatcher {
    sender: Sender<WatcherMessage>,
}

impl LibraryWatcher {
    pub(crate) fn new<H>(handler: H) -> Result<Self, LibraryWatcherError>
    where
        H: Fn(&LibraryChangeBatch) -> BatchDisposition + Send + 'static,
    {
        let (sender, receiver) = mpsc::channel();
        let event_sender = sender.clone();
        let watcher = RecommendedWatcher::new(
            move |event| {
                let _ = event_sender.send(WatcherMessage::Event(event));
            },
            Config::default(),
        )?;

        thread::Builder::new()
            .name("library-file-watcher".to_owned())
            .spawn(move || run_watcher_loop(watcher, receiver, handler))?;

        Ok(Self { sender })
    }

    pub(crate) fn set_folder(&self, folder: PathBuf) -> Result<(), LibraryWatcherError> {
        let (reply, response) = mpsc::sync_channel(1);
        self.sender
            .send(WatcherMessage::SetFolder { folder, reply })
            .map_err(|_| LibraryWatcherError::Stopped)?;
        response
            .recv()
            .map_err(|_| LibraryWatcherError::Stopped)??;
        Ok(())
    }
}

impl Drop for LibraryWatcher {
    fn drop(&mut self) {
        let _ = self.sender.send(WatcherMessage::Shutdown);
    }
}

fn run_watcher_loop<H>(
    mut watcher: RecommendedWatcher,
    receiver: Receiver<WatcherMessage>,
    handler: H,
) where
    H: Fn(&LibraryChangeBatch) -> BatchDisposition,
{
    let mut current_folder = None;
    let mut pending = PendingChanges::default();

    loop {
        let message = match pending.next_timeout() {
            Some(timeout) => receiver.recv_timeout(timeout),
            None => receiver.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };

        match message {
            Ok(WatcherMessage::SetFolder { folder, reply }) => {
                let result = replace_watched_folder(&mut watcher, &mut current_folder, folder);
                if result.is_ok() {
                    pending.clear();
                }
                let _ = reply.send(result);
            }
            Ok(WatcherMessage::Event(Ok(event))) => {
                if let Some(folder) = current_folder.as_ref() {
                    pending.record_event(event, folder);
                }
            }
            Ok(WatcherMessage::Event(Err(error))) => {
                if current_folder.is_some() {
                    pending.record_error(error);
                }
            }
            Ok(WatcherMessage::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {
                let Some(folder) = current_folder.clone() else {
                    pending.clear();
                    continue;
                };
                if pending.is_empty() {
                    pending.clear();
                    continue;
                }
                let batch = pending.take_batch(folder);
                if handler(&batch) == BatchDisposition::Retry {
                    pending.retry(batch);
                } else {
                    pending.clear();
                }
            }
        }
    }
}

fn replace_watched_folder(
    watcher: &mut RecommendedWatcher,
    current_folder: &mut Option<PathBuf>,
    folder: PathBuf,
) -> Result<(), notify::Error> {
    if current_folder.as_ref() == Some(&folder) {
        return Ok(());
    }

    let previous = current_folder.take();
    if let Some(previous) = previous.as_ref() {
        let _ = watcher.unwatch(previous);
    }
    if let Err(error) = watcher.watch(&folder, RecursiveMode::Recursive) {
        if let Some(previous) = previous {
            if watcher.watch(&previous, RecursiveMode::Recursive).is_ok() {
                *current_folder = Some(previous);
            }
        }
        return Err(error);
    }
    *current_folder = Some(folder);
    Ok(())
}

#[cfg(test)]
mod tests {
    use notify::event::ModifyKind;

    use super::*;

    #[test]
    fn events_outside_the_active_folder_should_not_schedule_a_batch() {
        let mut pending = PendingChanges::default();
        let folder = PathBuf::from("/music");
        let event = Event::new(EventKind::Modify(ModifyKind::Any))
            .add_path(PathBuf::from("/other/song.mp3"));

        pending.record_event(event, &folder);

        assert!(pending.next_timeout().is_none());
    }
}
