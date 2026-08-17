# Adaptive Audio Source routing, preparation, and downloads

Status: accepted

## Context

Online playback can resolve the same song through several imported Audio
Sources and several search channels. The previous policy tried Audio Sources
serially in a user-defined order. A slow or unavailable first source therefore
consumed its per-source timeout before a healthy source was tried. The player
cached song-specific failures for five minutes, but did not learn that an Audio
Source had recently succeeded for the same channel and quality.

Queue pagination was loaded ahead of time, but the next song's playback URL and
media were not prepared until the current song ended.

Online downloads used the same persisted selection mode but Automatic mode had
no learned ordering: it fell back to source name order for every item. Download
task events also contained only occasional full task snapshots, so the task bar
counted terminal files and ignored the byte progress already stored for active
files.

Mature routing systems combine the following mechanisms:

- Passive health observations from real traffic. Envoy describes outlier
  detection as passive health checking over consecutive failures, temporal
  success rate, and temporal latency. It temporarily ejects an unhealthy host,
  increases the ejection period after repeated failures, and later admits the
  host again for recovery checks. [Envoy outlier detection][1]
- Ordered fallback with temporary penalties. Pathway-based Content Steering
  selects the first non-penalized pathway and requires a penalized pathway to be
  reconsidered after enough time for recovery. [Pathway-based Content
  Steering, sections 4 and 7][2]
- Delayed request hedging for tail latency. gRPC sends the preferred request
  first, starts another request only after a hedge delay, accepts the first
  successful response, and cancels outstanding requests. It also warns that
  unbounded hedging adds backend load. [gRPC Request Hedging][3]
- Proximity-based preloading. Android Media3's `DefaultPreloadManager` treats a
  playlist as a one-dimensional list and prioritizes items by distance from the
  currently playing item. [Media3 preload manager][4]
- Bounded browser media preloading. `preload="auto"` can warm enough media for
  playback, but remains a browser hint and should be constrained by resource
  count and network cost. [web.dev media preload guidance][5]

## Decision

Fika exposes two persisted Audio Source selection modes.

### Automatic

The playback router owns session-scoped health for each
`Audio Source x channel x quality` route.

- A successful URL resolution and media probe records end-to-end latency using
  an exponentially weighted moving average and creates a recent-success
  affinity for ten minutes.
- A failure immediately lowers the route's rank. Two consecutive failures open
  a 30-second circuit. Repeated failures exponentially increase that interval,
  capped at five minutes.
- Health observations expire after 30 minutes. Expired circuits are admitted
  again automatically, and when every compatible route is penalized the router
  still permits a recovery attempt.
- Compatible Audio Sources are ranked from recent success, EWMA latency,
  failure history, candidate position, and quality fallback distance. Unknown
  routes retain an exploration score so a historical winner cannot monopolize
  playback forever.
- The best Audio Source starts immediately. At most one backup source starts
  after a dynamic 400-1200 ms hedge delay. A quick failure bypasses the delay.
  The first successful source wins and all other source and channel requests
  are cancelled.
- User-maintained Audio Source priority is hidden and ignored. Session health
  is intentionally not persisted across application restarts because imported
  third-party endpoints and network conditions become stale quickly.
- Playback and downloads share a process-scoped `channel -> Audio Source`
  preference for the most recent successful route. It expires after ten
  minutes and strongly biases both routers toward the same source for that
  channel. Playback refreshes it after the media URL probe succeeds; downloads
  refresh it only after the complete file has been persisted. Each router's
  own circuit breaker remains authoritative and can skip an unhealthy shared
  preference.

### Manual

The selected Audio Source remains first, followed by the persisted fallback
order. Source layers are attempted serially, preserving the previous explicit
control model. Runtime failures are still excluded for the current song so
playback recovery does not repeat the same URL.

### Next-track preparation

- Only the deterministic next item in sequential or repeat-all mode is
  prepared.
- Preparation starts 750 ms after the current item reaches `playing`, so it
  does not compete with initial playback startup.
- The task resolves the playback URL and uses a hidden audio element with
  `preload="auto"` to warm browser media state. It has one in-flight slot and is
  cancelled on queue, mode, quality, source, channel, or settings changes.
- A prepared result is consumed directly at the queue transition. Results
  older than 90 seconds are refreshed when playback enters the final 30
  seconds, limiting exposure to signed URL expiry.
- Preparation failures update route health but do not enter the song-specific
  negative cache. A foreground attempt can therefore retry after conditions
  change.
- Shuffle mode does not preload because there is no deterministic next item.

### Download routing and progress

- Download workers own a separate, process-scoped health router using the same
  `Audio Source x channel x quality` key, EWMA latency, recent-success affinity,
  and exponential temporary ejection policy as playback. Browser playback and
  Rust download observations are not mixed because their probes, clients, and
  cancellation lifecycles differ. Only the channel-level successful-source
  preference described above crosses that boundary.
- Automatic downloads start the highest-ranked source immediately and at most
  one backup after a dynamic 400-1200 ms delay. A quick primary failure starts
  the backup immediately; the first success cancels the other source layer.
- Manual downloads preserve the selected Audio Source captured by the task,
  followed by the configured fallback priority. Short-lived health does not
  reorder that explicit task snapshot.
- URL probe outcomes update route health. A failed media request, unsupported
  response type, broken stream, or empty response also penalizes the winning
  route, so a URL that probes successfully but cannot be downloaded does not
  remain preferred indefinitely.
- Workers emit a compact byte-progress event at most every 250 ms while keeping
  full task events for state transitions. Database updates and events are
  conditional on the item still being in `downloading`, preventing a late
  network read from reviving progress after pause or cancellation.
- Batch progress gives each item equal weight and includes the byte fraction of
  every concurrent active item. Unknown content lengths remain indeterminate
  and display transferred bytes without claiming an exact percentage.

## Consequences

- A recently successful source for a channel is normally selected first on the
  next playback or download, so both paths normally choose the same source.
  Repeated failures remain path-specific and stop an unhealthy source from
  consuming the full timeout on every track.
- Delayed hedging reduces tail latency without issuing every request to every
  Audio Source at once.
- Automatic mode is adaptive but not deterministic. The playback menu reports
  the Audio Source actually in use; users who need a fixed source can select
  Manual mode.
- Browser media preloading is best effort. The resolved URL still removes most
  transition work when the browser declines to retain buffered bytes.
- Download batches learn a working source after real traffic and expose smooth
  progress without repeatedly serializing the complete task and every track.
  The first unknown route can still be explored, while bounded hedging limits
  the initial tail latency and additional provider load.
- The current implementation does not persist or expose routing telemetry.
  Future tuning should be driven by measured startup latency, hedge rate,
  cancellation rate, and recovery success rather than by adding active probes
  to every playback.

[1]: https://www.envoyproxy.io/docs/envoy/latest/intro/arch_overview/upstream/outlier
[2]: https://datatracker.ietf.org/doc/draft-pantos-content-steering/
[3]: https://grpc.io/docs/guides/request-hedging/
[4]: https://developer.android.com/media/media3/exoplayer/preloading-media/preloadmanager
[5]: https://web.dev/articles/fast-playback-with-preload
