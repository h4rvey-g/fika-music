# Fika Music

Fika Music is a local-first music player that combines a personal music library with compatible online music source scripts.

## Language

**Track**:
A playable music item known to the app.
_Avoid_: Song, music item

**Local Track**:
A **Track** backed by a file on the user's device.
_Avoid_: Offline song, file track

**Remote Track**:
A **Track** represented by an online music source.
_Avoid_: Cloud song, online item

**Library**:
The user's indexed collection of known tracks and playlists.
_Avoid_: Collection, database

**Playlist**:
An ordered collection of tracks owned locally or by an online music service.
_Avoid_: List, queue

**Source Script**:
A JavaScript integration module compatible with the LX Music-style source model.
_Avoid_: Scraper, provider script

**Connector**:
A built-in integration with an online music service.
_Avoid_: Plugin, adapter

**NetEase API Basis**:
The upstream `NeteaseCloudMusicApiEnhanced/api-enhanced` project used as the reference or bridge for NetEase Cloud Music behavior.
_Avoid_: Official NetEase API

**Service Bridge**:
A host-managed integration component that gives Source Scripts controlled access to app-provided service behavior.
_Avoid_: Plugin sidecar, script daemon

**Source Runtime**:
The JavaScript execution environment used to run Source Scripts.
_Avoid_: Node runtime, browser runtime

**Account Ref**:
An opaque reference to a stored online-service account session.
_Avoid_: Cookie, token, password

**Capability**:
A permission category granted to a source script or connector.
_Avoid_: Access flag, scope

## Relationships

- A **Library** contains zero or more **Tracks**.
- A **Playlist** contains zero or more ordered **Tracks**.
- A **Track** may be a **Local Track** or a **Remote Track**.
- A **Source Script** exposes online music source behavior through granted **Capabilities**.
- A **Source Script** does not access local files directly; local file IO belongs to the app core.
- A **Source Script** may call a **Service Bridge** only through app-provided APIs.
- A **Source Script** may receive an **Account Ref**, but not raw account secrets.
- A **Connector** integrates one online music service without being user-installed.
- A **Service Bridge** is managed by the app, not bundled or launched by a **Source Script**.

## Example Dialogue

> **Dev:** "When a **Source Script** returns a **Remote Track**, do we add it to the **Library** automatically?"
> **Domain expert:** "No. It becomes part of the **Library** only when the user saves it, adds it to a **Playlist**, or plays it through a persisted history feature."

## Flagged Ambiguities

- "JS sources similar to LX Music" is resolved as direct LX Music-style **Source Script** compatibility for the first plugin platform.
