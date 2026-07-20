# Fika Music

Fika Music is a local-first music player that combines a personal music library with LX-compatible online music source providers.

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

**Source Provider**:
A Rust-native online music source module loaded by Fika's **Source Runtime**.
_Avoid_: Original LX JavaScript script, plugin, scraper

**LX Compatibility**:
Fika's core ability to model LX Music-style source actions and data contracts through Rust-native **Source Providers**.
_Avoid_: Running original LX JavaScript, plugin feature, plugin compatibility

**Plugin**:
A packaged, installable or bundled unit that contains a manifest and one or more Source Providers/assets.
_Avoid_: Source Runtime, LX Compatibility

**Plugin System**:
The app layer that installs, validates, enables/disables, permission-reviews, updates, and diagnoses Plugins.
_Avoid_: Source Runtime, LX Compatibility

**Connector**:
A built-in integration with an online music service.
_Avoid_: Plugin, adapter

**NetEase API Basis**:
The upstream `NeteaseCloudMusicApiEnhanced/api-enhanced` project used as the reference or bridge for NetEase Cloud Music behavior.
_Avoid_: Official NetEase API

**Service Bridge**:
A host-managed integration component that gives Source Providers controlled access to app-provided service behavior.
_Avoid_: Plugin sidecar, script daemon

**Source Runtime**:
The core Rust dispatcher used to initialize Source Providers, enforce Capabilities, collect diagnostics, and provide **LX Compatibility**.
_Avoid_: JavaScript runtime, Plugin runtime, Node runtime, browser runtime

**Account Ref**:
An opaque reference to a stored online-service account session.
_Avoid_: Cookie, token, password

**Capability**:
A permission category granted to a Source Provider or Connector.
_Avoid_: Access flag, scope

## Relationships

- A **Library** contains zero or more **Tracks**.
- A **Playlist** contains zero or more ordered **Tracks**.
- A **Track** may be a **Local Track** or a **Remote Track**.
- A **Source Provider** exposes online music source behavior through granted **Capabilities**.
- A **Source Provider** does not access local files directly; local file IO belongs to the app core.
- A **Source Provider** may call a **Service Bridge** only through app-provided APIs.
- A **Source Provider** may receive an **Account Ref**, but not raw account secrets.
- **LX Compatibility** is a core app capability provided by the **Source Runtime**, not a plugin feature.
- A **Plugin** may package one or more **Source Providers** plus metadata, assets, and declared **Capabilities**.
- The **Plugin System** manages **Plugins**, but it does not implement **LX Compatibility** itself.
- A **Connector** integrates one online music service without being user-installed.
- A **Service Bridge** is managed by the app, not bundled or launched by a **Source Provider**.

## Example Dialogue

> **Dev:** "When a **Source Provider** returns a **Remote Track**, do we add it to the **Library** automatically?"
> **Domain expert:** "No. It becomes part of the **Library** only when the user saves it, adds it to a **Playlist**, or plays it through a persisted history feature."

## Flagged Ambiguities

- "LX compatibility" is resolved as Rust-native **Source Provider** implementations that follow LX Music-style actions and data contracts, not execution of original LX JavaScript.
