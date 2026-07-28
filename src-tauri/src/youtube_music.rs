use crate::source_runtime::{
    self, JsonScalar, LyricResponse, SourceAction, SourceAlbumSearchResponse,
    SourceAlbumSearchResult, SourceArtistBiography, SourceArtistBiographySection,
    SourceArtistSearchResponse, SourceArtistSearchResult, SourceCapability, SourceEntityRef,
    SourceHttpRequest, SourceInfo, SourcePlaylistSearchResponse, SourcePlaylistSearchResult,
    SourceProvider, SourceRequest, SourceResponse, SourceRuntimeApiVersion, SourceRuntimeContext,
    SourceRuntimeError, SourceSearchResponse, SourceSearchResult, SourceSuggestionsResponse,
};
use serde_json::{json, Value as JsonValue};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Mutex;
use url::Url;

pub const YOUTUBE_MUSIC_PLUGIN_ID: &str = "fika.youtube-music";
pub const YOUTUBE_MUSIC_PROVIDER_ID: &str = "fika-youtube-music";
pub const YOUTUBE_MUSIC_PROVIDER_ENTRYPOINT: &str = "builtin:youtube-music";
pub const YOUTUBE_MUSIC_SOURCE_ID: &str = "yt";
pub const YOUTUBE_MUSIC_PROVIDER_API_VERSION: SourceRuntimeApiVersion =
    SourceRuntimeApiVersion::new(1, 4);

const YOUTUBE_MUSIC_ORIGIN: &str = "https://music.youtube.com";
const YOUTUBE_MUSIC_API_BASE: &str = "https://music.youtube.com/youtubei/v1/";
const YOUTUBE_BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";
const MAX_CONTINUATION_REQUESTS: usize = 8;
const MAX_PAGED_RESULTS: usize = 500;

const SEARCH_FILTER_SONGS: &str = "EgWKAQIIAWoMEA4QChADEAQQCRAF";
const SEARCH_FILTER_ARTISTS: &str = "EgWKAQIgAWoMEA4QChADEAQQCRAF";
const SEARCH_FILTER_ALBUMS: &str = "EgWKAQIYAWoMEA4QChADEAQQCRAF";
const SEARCH_FILTER_PLAYLISTS: &str = "Eg-KAQwIABAAGAAgACgBMAFqChAEEAMQCRAFEAo%3D";

#[derive(Debug, Clone, PartialEq, Eq)]
struct WebClientConfig {
    api_key: String,
    client_version: String,
    visitor_data: Option<String>,
}

#[derive(Debug)]
struct RendererPage {
    items: Vec<JsonValue>,
    is_end: bool,
}

pub struct YoutubeMusicSourceProvider {
    id: String,
    capabilities: BTreeSet<SourceCapability>,
    web_config: Mutex<Option<WebClientConfig>>,
}

impl fmt::Debug for YoutubeMusicSourceProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("YoutubeMusicSourceProvider")
            .field("id", &self.id)
            .field("capabilities", &self.capabilities)
            .finish_non_exhaustive()
    }
}

impl YoutubeMusicSourceProvider {
    pub fn new(id: String, capabilities: BTreeSet<SourceCapability>) -> Self {
        Self {
            id,
            capabilities,
            web_config: Mutex::new(None),
        }
    }

    fn request(
        &self,
        context: &mut SourceRuntimeContext,
        endpoint: &str,
        payload: JsonValue,
        continuation: Option<&str>,
        operation: &str,
    ) -> Result<JsonValue, SourceRuntimeError> {
        let mut config = self.web_config(context, operation)?;
        let mut response = self.send_request(
            context,
            endpoint,
            payload.clone(),
            continuation,
            operation,
            &config,
        )?;

        if matches!(response.status, 400 | 403) {
            self.clear_web_config(context)?;
            config = self.web_config(context, operation)?;
            response =
                self.send_request(context, endpoint, payload, continuation, operation, &config)?;
        }

        if !response.is_success() {
            let message = serde_json::from_slice::<JsonValue>(&response.body)
                .ok()
                .and_then(|body| {
                    string_at(&body, &["error", "message"])
                        .or_else(|| string_at(&body, &["playabilityStatus", "reason"]))
                })
                .unwrap_or_else(|| format!("HTTP {}", response.status));
            return Err(context.provider_error_with_code(
                "youtube-api-failure",
                format!("{operation} failed: {message}"),
            ));
        }

        serde_json::from_slice(&response.body).map_err(|error| {
            context.provider_error_with_code(
                "invalid-response",
                format!("{operation} returned invalid JSON: {error}"),
            )
        })
    }

    fn send_request(
        &self,
        context: &mut SourceRuntimeContext,
        endpoint: &str,
        payload: JsonValue,
        continuation: Option<&str>,
        operation: &str,
        config: &WebClientConfig,
    ) -> Result<source_runtime::SourceHttpResponse, SourceRuntimeError> {
        let mut url = Url::parse(YOUTUBE_MUSIC_API_BASE)
            .and_then(|base| base.join(endpoint))
            .map_err(|error| {
                context.provider_error_with_code(
                    "invalid-request",
                    format!("could not construct YouTube Music endpoint: {error}"),
                )
            })?;
        url.query_pairs_mut()
            .append_pair("alt", "json")
            .append_pair("key", &config.api_key);
        if let Some(token) = continuation {
            url.query_pairs_mut()
                .append_pair("ctoken", token)
                .append_pair("continuation", token);
        }

        let mut body = payload.as_object().cloned().ok_or_else(|| {
            context.provider_error_with_code(
                "invalid-request",
                "YouTube Music request body must be a JSON object",
            )
        })?;
        let mut client = json!({
            "clientName": "WEB_REMIX",
            "clientVersion": config.client_version,
            "hl": "en",
            "gl": "US"
        });
        if let Some(visitor_data) = &config.visitor_data {
            client
                .as_object_mut()
                .expect("client context is an object")
                .insert("visitorData".to_owned(), json!(visitor_data));
        }
        body.insert("context".to_owned(), json!({ "client": client }));

        let mut request = SourceHttpRequest::post_json(url.to_string(), JsonValue::Object(body));
        request
            .headers
            .insert("Origin".to_owned(), YOUTUBE_MUSIC_ORIGIN.to_owned());
        request.headers.insert(
            "Content-Type".to_owned(),
            "application/json; charset=utf-8".to_owned(),
        );
        request.headers.insert(
            "User-Agent".to_owned(),
            YOUTUBE_BROWSER_USER_AGENT.to_owned(),
        );
        if let Some(visitor_data) = &config.visitor_data {
            request
                .headers
                .insert("X-Goog-Visitor-Id".to_owned(), visitor_data.clone());
        }
        context.http_request(request, operation)
    }

    fn web_config(
        &self,
        context: &mut SourceRuntimeContext,
        operation: &str,
    ) -> Result<WebClientConfig, SourceRuntimeError> {
        let cached = self
            .web_config
            .lock()
            .map_err(|_| context.provider_error("YouTube Music client config lock was poisoned"))?
            .clone();
        if let Some(config) = cached {
            return Ok(config);
        }

        let mut request = SourceHttpRequest::get(format!("{YOUTUBE_MUSIC_ORIGIN}/"));
        request.headers.insert(
            "User-Agent".to_owned(),
            YOUTUBE_BROWSER_USER_AGENT.to_owned(),
        );
        let response = context.http_request(
            request,
            &format!("bootstrap YouTube Music before {operation}"),
        )?;
        if !response.is_success() {
            return Err(context.provider_error_with_code(
                "youtube-bootstrap-failure",
                format!(
                    "YouTube Music bootstrap failed with HTTP {}",
                    response.status
                ),
            ));
        }
        let html = String::from_utf8(response.body).map_err(|error| {
            context.provider_error_with_code(
                "invalid-response",
                format!("YouTube Music bootstrap was not UTF-8: {error}"),
            )
        })?;
        let config = parse_web_client_config(&html).ok_or_else(|| {
            context.provider_error_with_code(
                "invalid-response",
                "YouTube Music bootstrap did not expose an InnerTube client config",
            )
        })?;
        *self.web_config.lock().map_err(|_| {
            context.provider_error("YouTube Music client config lock was poisoned")
        })? = Some(config.clone());
        Ok(config)
    }

    fn clear_web_config(
        &self,
        context: &mut SourceRuntimeContext,
    ) -> Result<(), SourceRuntimeError> {
        *self.web_config.lock().map_err(|_| {
            context.provider_error("YouTube Music client config lock was poisoned")
        })? = None;
        Ok(())
    }

    fn search_page(
        &self,
        context: &mut SourceRuntimeContext,
        keyword: &str,
        filter: &str,
        page: u64,
        page_size: u64,
        operation: &str,
    ) -> Result<RendererPage, SourceRuntimeError> {
        let body = json!({ "query": keyword, "params": filter });
        self.renderer_page(context, "search", body, page, page_size, operation)
    }

    fn browse_page(
        &self,
        context: &mut SourceRuntimeContext,
        browse_id: &str,
        page: u64,
        page_size: u64,
        operation: &str,
    ) -> Result<(JsonValue, RendererPage), SourceRuntimeError> {
        let body = json!({ "browseId": browse_id });
        let first = self.request(context, "browse", body.clone(), None, operation)?;
        let page = self.renderer_page_from_first(
            context,
            "browse",
            body,
            first.clone(),
            page,
            page_size,
            operation,
        )?;
        Ok((first, page))
    }

    fn renderer_page(
        &self,
        context: &mut SourceRuntimeContext,
        endpoint: &str,
        body: JsonValue,
        page: u64,
        page_size: u64,
        operation: &str,
    ) -> Result<RendererPage, SourceRuntimeError> {
        let first = self.request(context, endpoint, body.clone(), None, operation)?;
        self.renderer_page_from_first(context, endpoint, body, first, page, page_size, operation)
    }

    #[allow(clippy::too_many_arguments)]
    fn renderer_page_from_first(
        &self,
        context: &mut SourceRuntimeContext,
        endpoint: &str,
        body: JsonValue,
        first: JsonValue,
        page: u64,
        page_size: u64,
        operation: &str,
    ) -> Result<RendererPage, SourceRuntimeError> {
        let start = page.saturating_sub(1).saturating_mul(page_size) as usize;
        let target = page.saturating_mul(page_size).min(MAX_PAGED_RESULTS as u64) as usize;
        let mut response = first;
        let mut items = Vec::new();
        collect_values_for_key(&response, "musicResponsiveListItemRenderer", &mut items);
        let mut continuation = find_continuation_token(&response);
        let mut request_count = 0;
        while items.len() < target
            && continuation.is_some()
            && request_count < MAX_CONTINUATION_REQUESTS
        {
            context.ensure_not_cancelled(operation)?;
            response = self.request(
                context,
                endpoint,
                body.clone(),
                continuation.as_deref(),
                operation,
            )?;
            collect_values_for_key(&response, "musicResponsiveListItemRenderer", &mut items);
            continuation = find_continuation_token(&response);
            request_count += 1;
        }
        let is_end = continuation.is_none() && target >= items.len();
        let page_items = items
            .into_iter()
            .skip(start)
            .take(page_size as usize)
            .collect();
        Ok(RendererPage {
            items: page_items,
            is_end,
        })
    }

    fn artist_response(
        &self,
        context: &mut SourceRuntimeContext,
        artist: &SourceEntityRef,
        operation: &str,
    ) -> Result<JsonValue, SourceRuntimeError> {
        let browse_id = entity_browse_id(artist).ok_or_else(|| {
            context
                .provider_error_with_code("invalid-artist", "YouTube Music artist has no browseId")
        })?;
        self.request(
            context,
            "browse",
            json!({ "browseId": browse_id.trim_start_matches("MPLA") }),
            None,
            operation,
        )
    }

    fn search_tracks(
        &self,
        context: &mut SourceRuntimeContext,
        keyword: &str,
        page: u64,
        page_size: u64,
    ) -> Result<SourceSearchResponse, SourceRuntimeError> {
        let page = self.search_page(
            context,
            keyword,
            SEARCH_FILTER_SONGS,
            page,
            page_size,
            "search YouTube Music tracks",
        )?;
        let list = page
            .items
            .iter()
            .filter_map(|item| track_from_renderer(item, None, None, None))
            .collect();
        Ok(SourceSearchResponse {
            is_end: page.is_end,
            total: None,
            list,
        })
    }

    fn search_artists(
        &self,
        context: &mut SourceRuntimeContext,
        keyword: &str,
        page: u64,
        page_size: u64,
    ) -> Result<SourceArtistSearchResponse, SourceRuntimeError> {
        let page = self.search_page(
            context,
            keyword,
            SEARCH_FILTER_ARTISTS,
            page,
            page_size,
            "search YouTube Music artists",
        )?;
        let list = page.items.iter().filter_map(artist_from_renderer).collect();
        Ok(SourceArtistSearchResponse {
            is_end: page.is_end,
            total: None,
            list,
        })
    }

    fn search_albums(
        &self,
        context: &mut SourceRuntimeContext,
        keyword: &str,
        page: u64,
        page_size: u64,
    ) -> Result<SourceAlbumSearchResponse, SourceRuntimeError> {
        let page = self.search_page(
            context,
            keyword,
            SEARCH_FILTER_ALBUMS,
            page,
            page_size,
            "search YouTube Music albums",
        )?;
        let list = page
            .items
            .iter()
            .filter_map(album_from_responsive_renderer)
            .collect();
        Ok(SourceAlbumSearchResponse {
            is_end: page.is_end,
            total: None,
            list,
        })
    }

    fn search_playlists(
        &self,
        context: &mut SourceRuntimeContext,
        keyword: &str,
        page: u64,
        page_size: u64,
    ) -> Result<SourcePlaylistSearchResponse, SourceRuntimeError> {
        let page = self.search_page(
            context,
            keyword,
            SEARCH_FILTER_PLAYLISTS,
            page,
            page_size,
            "search YouTube Music playlists",
        )?;
        let list = page
            .items
            .iter()
            .filter_map(playlist_from_renderer)
            .collect();
        Ok(SourcePlaylistSearchResponse {
            is_end: page.is_end,
            total: None,
            list,
        })
    }

    fn suggestions(
        &self,
        context: &mut SourceRuntimeContext,
        keyword: &str,
        limit: u64,
    ) -> Result<SourceSuggestionsResponse, SourceRuntimeError> {
        let response = self.request(
            context,
            "music/get_search_suggestions",
            json!({ "input": keyword }),
            None,
            "fetch YouTube Music search suggestions",
        )?;
        let mut renderers = Vec::new();
        collect_values_for_key(&response, "searchSuggestionRenderer", &mut renderers);
        collect_values_for_key(&response, "historySuggestionRenderer", &mut renderers);
        let mut list = Vec::new();
        for renderer in renderers {
            let suggestion = string_at(
                &renderer,
                &["navigationEndpoint", "searchEndpoint", "query"],
            )
            .or_else(|| text_at(&renderer, &["suggestion"]));
            if let Some(suggestion) = suggestion {
                push_unique(&mut list, suggestion);
            }
        }
        list.truncate(limit as usize);
        Ok(SourceSuggestionsResponse { list })
    }

    fn artist_top_tracks(
        &self,
        context: &mut SourceRuntimeContext,
        artist: &SourceEntityRef,
        limit: u64,
    ) -> Result<SourceSearchResponse, SourceRuntimeError> {
        let response = self.artist_response(context, artist, "read YouTube Music artist")?;
        let artist_name = artist_header_name(&response);
        let mut renderers = Vec::new();
        collect_values_for_key(&response, "musicResponsiveListItemRenderer", &mut renderers);
        let mut list = renderers
            .iter()
            .filter_map(|item| track_from_renderer(item, artist_name.as_deref(), None, None))
            .collect::<Vec<_>>();
        list.truncate(limit as usize);
        Ok(SourceSearchResponse {
            is_end: true,
            total: Some(list.len() as u64),
            list,
        })
    }

    fn artist_albums(
        &self,
        context: &mut SourceRuntimeContext,
        artist: &SourceEntityRef,
        page: u64,
        page_size: u64,
    ) -> Result<SourceAlbumSearchResponse, SourceRuntimeError> {
        let response = self.artist_response(context, artist, "read YouTube Music artist albums")?;
        let artist_name = artist_header_name(&response).unwrap_or_default();
        let mut renderers = Vec::new();
        collect_values_for_key(&response, "musicTwoRowItemRenderer", &mut renderers);
        let albums = renderers
            .iter()
            .filter_map(|item| album_from_two_row_renderer(item, &artist_name))
            .collect::<Vec<_>>();
        let total = albums.len() as u64;
        let (list, is_end) = paginate(albums, page, page_size);
        Ok(SourceAlbumSearchResponse {
            is_end,
            total: Some(total),
            list,
        })
    }

    fn artist_biography(
        &self,
        context: &mut SourceRuntimeContext,
        artist: &SourceEntityRef,
    ) -> Result<SourceArtistBiography, SourceRuntimeError> {
        let response =
            self.artist_response(context, artist, "read YouTube Music artist biography")?;
        let description = find_value_for_key(&response, "musicDescriptionShelfRenderer")
            .and_then(|renderer| text_at(renderer, &["description"]))
            .or_else(|| {
                find_value_for_key(&response, "musicImmersiveHeaderRenderer")
                    .and_then(|header| text_at(header, &["description"]))
            });
        let sections = description
            .as_ref()
            .map(|text| {
                vec![SourceArtistBiographySection {
                    title: "About".to_owned(),
                    text: text.clone(),
                }]
            })
            .unwrap_or_default();
        Ok(SourceArtistBiography {
            summary: description,
            sections,
        })
    }

    fn album_tracks(
        &self,
        context: &mut SourceRuntimeContext,
        album: &SourceEntityRef,
        page: u64,
        page_size: u64,
    ) -> Result<SourceSearchResponse, SourceRuntimeError> {
        let browse_id = entity_browse_id(album).ok_or_else(|| {
            context.provider_error_with_code("invalid-album", "YouTube Music album has no browseId")
        })?;
        let (response, page) = self.browse_page(
            context,
            &browse_id,
            page,
            page_size,
            "read YouTube Music album",
        )?;
        let header = browse_header(&response);
        let list = page
            .items
            .iter()
            .filter_map(|item| {
                track_from_renderer(
                    item,
                    header.artist.as_deref(),
                    header.title.as_deref(),
                    header.cover_url.as_deref(),
                )
            })
            .collect();
        Ok(SourceSearchResponse {
            is_end: page.is_end,
            total: header.track_count,
            list,
        })
    }

    fn playlist_tracks(
        &self,
        context: &mut SourceRuntimeContext,
        playlist: &SourceEntityRef,
        page: u64,
        page_size: u64,
    ) -> Result<SourceSearchResponse, SourceRuntimeError> {
        let playlist_id = entity_playlist_id(playlist).ok_or_else(|| {
            context.provider_error_with_code(
                "invalid-playlist",
                "YouTube Music playlist has no playlistId",
            )
        })?;
        let browse_id = if playlist_id.starts_with("VL") {
            playlist_id
        } else {
            format!("VL{playlist_id}")
        };
        let (response, page) = self.browse_page(
            context,
            &browse_id,
            page,
            page_size,
            "read YouTube Music public playlist",
        )?;
        let header = browse_header(&response);
        let list = page
            .items
            .iter()
            .filter_map(|item| track_from_renderer(item, None, None, None))
            .collect();
        Ok(SourceSearchResponse {
            is_end: page.is_end,
            total: header.track_count,
            list,
        })
    }

    fn lyrics(
        &self,
        context: &mut SourceRuntimeContext,
        music_info: &JsonValue,
    ) -> Result<LyricResponse, SourceRuntimeError> {
        let video_id = music_info_video_id(music_info).ok_or_else(|| {
            context.provider_error_with_code("invalid-track", "YouTube Music track has no videoId")
        })?;
        let next = self.request(
            context,
            "next",
            json!({
                "videoId": video_id,
                "playlistId": format!("RDAMVM{video_id}"),
                "isAudioOnly": true,
                "enablePersistentPlaylistPanel": true,
                "tunerSettingValue": "AUTOMIX_SETTING_NORMAL"
            }),
            None,
            "locate YouTube Music lyrics",
        )?;
        let Some(browse_id) = find_browse_id_for_page_type(&next, "MUSIC_PAGE_TYPE_TRACK_LYRICS")
        else {
            return Ok(empty_lyrics());
        };
        let response = self.request(
            context,
            "browse",
            json!({ "browseId": browse_id }),
            None,
            "read YouTube Music lyrics",
        )?;
        let lyric = find_value_for_key(&response, "musicDescriptionShelfRenderer")
            .and_then(|renderer| text_at(renderer, &["description"]));
        Ok(LyricResponse {
            lyric,
            tlyric: None,
            rlyric: None,
            lxlyric: None,
        })
    }
}

impl SourceProvider for YoutubeMusicSourceProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn api_version(&self) -> SourceRuntimeApiVersion {
        YOUTUBE_MUSIC_PROVIDER_API_VERSION
    }

    fn required_capabilities(&self) -> BTreeSet<SourceCapability> {
        self.capabilities.clone()
    }

    fn initialize(
        &self,
        context: &mut SourceRuntimeContext,
    ) -> Result<BTreeMap<String, SourceInfo>, SourceRuntimeError> {
        context.info("initialized bundled YouTube Music catalog Provider");
        Ok(BTreeMap::from([(
            YOUTUBE_MUSIC_SOURCE_ID.to_owned(),
            source_runtime::lx_music_source(
                YOUTUBE_MUSIC_SOURCE_ID,
                "YouTube Music",
                vec![
                    SourceAction::MusicSearch,
                    SourceAction::ArtistSearch,
                    SourceAction::AlbumSearch,
                    SourceAction::PlaylistSearch,
                    SourceAction::SearchSuggestions,
                    SourceAction::ArtistTopTracks,
                    SourceAction::ArtistAlbums,
                    SourceAction::ArtistBiography,
                    SourceAction::AlbumRead,
                    SourceAction::PlaylistReadPublic,
                    SourceAction::Lyric,
                    SourceAction::Pic,
                ],
                Vec::new(),
            ),
        )]))
    }

    fn handle_request(
        &self,
        context: &mut SourceRuntimeContext,
        request: SourceRequest,
    ) -> Result<SourceResponse, SourceRuntimeError> {
        match request {
            SourceRequest::MusicSearch {
                keyword,
                page,
                page_size,
                ..
            } => self
                .search_tracks(context, &keyword, page, page_size)
                .map(SourceResponse::MusicSearch),
            SourceRequest::ArtistSearch {
                keyword,
                page,
                page_size,
                ..
            } => self
                .search_artists(context, &keyword, page, page_size)
                .map(SourceResponse::ArtistSearch),
            SourceRequest::AlbumSearch {
                keyword,
                page,
                page_size,
                ..
            } => self
                .search_albums(context, &keyword, page, page_size)
                .map(SourceResponse::AlbumSearch),
            SourceRequest::PlaylistSearch {
                keyword,
                page,
                page_size,
                ..
            } => self
                .search_playlists(context, &keyword, page, page_size)
                .map(SourceResponse::PlaylistSearch),
            SourceRequest::SearchSuggestions { keyword, limit, .. } => self
                .suggestions(context, &keyword, limit)
                .map(SourceResponse::SearchSuggestions),
            SourceRequest::ArtistTopTracks { artist, limit, .. } => self
                .artist_top_tracks(context, &artist, limit)
                .map(SourceResponse::ArtistTopTracks),
            SourceRequest::ArtistAlbums {
                artist,
                page,
                page_size,
                ..
            } => self
                .artist_albums(context, &artist, page, page_size)
                .map(SourceResponse::ArtistAlbums),
            SourceRequest::ArtistBiography { artist, .. } => self
                .artist_biography(context, &artist)
                .map(SourceResponse::ArtistBiography),
            SourceRequest::AlbumRead {
                album,
                page,
                page_size,
                ..
            } => self
                .album_tracks(context, &album, page, page_size)
                .map(SourceResponse::AlbumRead),
            SourceRequest::PlaylistReadPublic {
                playlist,
                page,
                page_size,
                ..
            } => self
                .playlist_tracks(context, &playlist, page, page_size)
                .map(SourceResponse::PlaylistReadPublic),
            SourceRequest::Lyric { music_info, .. } => {
                self.lyrics(context, &music_info).map(SourceResponse::Lyric)
            }
            SourceRequest::Pic { music_info, .. } => {
                let cover = music_info
                    .get("coverUrl")
                    .or_else(|| music_info.get("thumbnailUrl"))
                    .and_then(JsonValue::as_str)
                    .map(str::to_owned)
                    .or_else(|| {
                        music_info_video_id(&music_info)
                            .map(|id| format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg"))
                    })
                    .ok_or_else(|| context.provider_error("YouTube Music track has no artwork"))?;
                Ok(SourceResponse::Pic(cover))
            }
            request => Err(context.unsupported_action(request.source(), request.action())),
        }
    }
}

#[derive(Debug, Default)]
struct BrowseHeader {
    title: Option<String>,
    artist: Option<String>,
    cover_url: Option<String>,
    track_count: Option<u64>,
}

fn parse_web_client_config(html: &str) -> Option<WebClientConfig> {
    Some(WebClientConfig {
        api_key: extract_config_string(html, "INNERTUBE_API_KEY")?,
        client_version: extract_config_string(html, "INNERTUBE_CLIENT_VERSION")?,
        visitor_data: extract_config_string(html, "VISITOR_DATA"),
    })
}

fn extract_config_string(input: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let mut remainder = input;
    while let Some(index) = remainder.find(&needle) {
        let after_key = &remainder[index + needle.len()..];
        let after_colon = after_key.strip_prefix(':').or_else(|| {
            let trimmed = after_key.trim_start();
            trimmed.strip_prefix(':')
        });
        if let Some(after_colon) = after_colon {
            let value = after_colon.trim_start();
            if value.starts_with('"') {
                if let Some(end) = json_string_end(value) {
                    if let Ok(parsed) = serde_json::from_str::<String>(&value[..=end]) {
                        return Some(parsed);
                    }
                }
            }
        }
        remainder = &after_key[after_key.len().min(1)..];
    }
    None
}

fn json_string_end(value: &str) -> Option<usize> {
    let mut escaped = false;
    for (index, character) in value.char_indices().skip(1) {
        if character == '"' && !escaped {
            return Some(index);
        }
        if character == '\\' {
            escaped = !escaped;
        } else {
            escaped = false;
        }
    }
    None
}

fn track_from_renderer(
    item: &JsonValue,
    default_artist: Option<&str>,
    default_album: Option<&str>,
    default_cover: Option<&str>,
) -> Option<SourceSearchResult> {
    let video_id = renderer_video_id(item)?;
    let title = flex_text(item, 0)?;
    let metadata = flex_runs(item, 1);
    let artist_names = run_names_for_page_types(
        metadata,
        &["MUSIC_PAGE_TYPE_ARTIST", "MUSIC_PAGE_TYPE_USER_CHANNEL"],
    );
    let artist = if artist_names.is_empty() {
        default_artist
            .map(str::to_owned)
            .or_else(|| fallback_metadata_name(metadata))
            .unwrap_or_else(|| "Unknown artist".to_owned())
    } else {
        artist_names.join(", ")
    };
    let album = run_name_for_page_type(metadata, "MUSIC_PAGE_TYPE_ALBUM")
        .or_else(|| default_album.map(str::to_owned));
    let album_browse_id = run_browse_id_for_page_type(metadata, "MUSIC_PAGE_TYPE_ALBUM");
    let duration_seconds = fixed_text(item, 0)
        .or_else(|| {
            metadata_texts(metadata)
                .into_iter()
                .find(|text| parse_duration(text).is_some())
        })
        .and_then(|value| parse_duration(&value));
    let cover_url = best_thumbnail_url(item).or_else(|| default_cover.map(str::to_owned));
    let track_number = item
        .get("index")
        .and_then(|index| text_at(index, &[]))
        .and_then(|value| value.trim().parse::<u32>().ok());
    let mut platform_ids =
        BTreeMap::from([("videoId".to_owned(), JsonScalar::String(video_id.clone()))]);
    if let Some(album_id) = &album_browse_id {
        platform_ids.insert(
            "albumBrowseId".to_owned(),
            JsonScalar::String(album_id.clone()),
        );
    }
    let raw_info = json!({
        "videoId": video_id,
        "albumBrowseId": album_browse_id,
        "coverUrl": cover_url
    });
    Some(SourceSearchResult {
        id: video_id,
        source: YOUTUBE_MUSIC_SOURCE_ID.to_owned(),
        title,
        artist,
        album,
        duration_seconds,
        cover_url,
        track_number,
        disc_number: None,
        platform_ids,
        raw_info,
    })
}

fn artist_from_renderer(item: &JsonValue) -> Option<SourceArtistSearchResult> {
    let id = renderer_browse_id(item)?;
    if !id.starts_with("UC") && !id.starts_with("MPLA") {
        return None;
    }
    let name = flex_text(item, 0)?;
    Some(SourceArtistSearchResult {
        id: id.clone(),
        source: YOUTUBE_MUSIC_SOURCE_ID.to_owned(),
        name,
        cover_url: best_thumbnail_url(item),
        platform_ids: browse_platform_ids(&id),
        raw_info: json!({ "browseId": id }),
    })
}

fn album_from_responsive_renderer(item: &JsonValue) -> Option<SourceAlbumSearchResult> {
    let id = renderer_browse_id(item)?;
    if !id.starts_with("MPRE") {
        return None;
    }
    let title = flex_text(item, 0)?;
    let metadata = flex_runs(item, 1);
    let artist = run_names_for_page_types(
        metadata,
        &["MUSIC_PAGE_TYPE_ARTIST", "MUSIC_PAGE_TYPE_USER_CHANNEL"],
    )
    .join(", ");
    let playlist_id = find_string_for_key(item, "playlistId");
    Some(album_result(
        id,
        title,
        artist,
        metadata,
        best_thumbnail_url(item),
        playlist_id,
    ))
}

fn album_from_two_row_renderer(
    item: &JsonValue,
    default_artist: &str,
) -> Option<SourceAlbumSearchResult> {
    let id = string_at(item, &["navigationEndpoint", "browseEndpoint", "browseId"])?;
    if !id.starts_with("MPRE") {
        return None;
    }
    let title = text_at(item, &["title"])?;
    let metadata = runs_at(item, &["subtitle"]);
    let artists = run_names_for_page_types(
        metadata,
        &["MUSIC_PAGE_TYPE_ARTIST", "MUSIC_PAGE_TYPE_USER_CHANNEL"],
    );
    let artist = if artists.is_empty() {
        default_artist.to_owned()
    } else {
        artists.join(", ")
    };
    let playlist_id = find_string_for_key(item, "playlistId");
    Some(album_result(
        id,
        title,
        artist,
        metadata,
        best_thumbnail_url(item),
        playlist_id,
    ))
}

fn album_result(
    id: String,
    title: String,
    artist: String,
    metadata: &[JsonValue],
    cover_url: Option<String>,
    playlist_id: Option<String>,
) -> SourceAlbumSearchResult {
    let release_year = metadata_texts(metadata)
        .into_iter()
        .find_map(|text| parse_year(&text));
    let mut platform_ids = browse_platform_ids(&id);
    if let Some(playlist_id) = &playlist_id {
        platform_ids.insert(
            "playlistId".to_owned(),
            JsonScalar::String(playlist_id.clone()),
        );
    }
    SourceAlbumSearchResult {
        id: id.clone(),
        source: YOUTUBE_MUSIC_SOURCE_ID.to_owned(),
        title,
        artist,
        release_year,
        cover_url,
        track_count: None,
        platform_ids,
        raw_info: json!({ "browseId": id, "playlistId": playlist_id }),
    }
}

fn playlist_from_renderer(item: &JsonValue) -> Option<SourcePlaylistSearchResult> {
    let browse_id = renderer_browse_id(item)?;
    if !browse_id.starts_with("VL") && !browse_id.starts_with("PL") && !browse_id.starts_with("RD")
    {
        return None;
    }
    let playlist_id = browse_id
        .strip_prefix("VL")
        .unwrap_or(&browse_id)
        .to_owned();
    let metadata = flex_runs(item, 1);
    let owner_names = run_names_for_page_types(
        metadata,
        &["MUSIC_PAGE_TYPE_ARTIST", "MUSIC_PAGE_TYPE_USER_CHANNEL"],
    );
    let track_count = metadata_texts(metadata)
        .into_iter()
        .find_map(|text| parse_count(&text));
    Some(SourcePlaylistSearchResult {
        id: playlist_id.clone(),
        source: YOUTUBE_MUSIC_SOURCE_ID.to_owned(),
        name: flex_text(item, 0)?,
        description: None,
        cover_url: best_thumbnail_url(item),
        track_count,
        owner_name: (!owner_names.is_empty()).then(|| owner_names.join(", ")),
        platform_ids: BTreeMap::from([(
            "playlistId".to_owned(),
            JsonScalar::String(playlist_id.clone()),
        )]),
        raw_info: json!({ "browseId": browse_id, "playlistId": playlist_id }),
    })
}

fn browse_header(response: &JsonValue) -> BrowseHeader {
    let header = find_value_for_key(response, "musicResponsiveHeaderRenderer")
        .or_else(|| find_value_for_key(response, "musicDetailHeaderRenderer"));
    let Some(header) = header else {
        return BrowseHeader::default();
    };
    let title = text_at(header, &["title"]);
    let strapline = runs_at(header, &["straplineTextOne"]);
    let subtitle = runs_at(header, &["subtitle"]);
    let artist_names = run_names_for_page_types(
        if strapline.is_empty() {
            subtitle
        } else {
            strapline
        },
        &["MUSIC_PAGE_TYPE_ARTIST", "MUSIC_PAGE_TYPE_USER_CHANNEL"],
    );
    let track_count = text_at(header, &["secondSubtitle"]).and_then(|text| parse_count(&text));
    BrowseHeader {
        title,
        artist: (!artist_names.is_empty()).then(|| artist_names.join(", ")),
        cover_url: best_thumbnail_url(header),
        track_count,
    }
}

fn artist_header_name(response: &JsonValue) -> Option<String> {
    find_value_for_key(response, "musicImmersiveHeaderRenderer")
        .or_else(|| find_value_for_key(response, "musicVisualHeaderRenderer"))
        .and_then(|header| text_at(header, &["title"]))
}

fn entity_browse_id(entity: &SourceEntityRef) -> Option<String> {
    scalar_string(&entity.platform_ids, "browseId")
        .or_else(|| json_object_string(&entity.raw_info, "browseId"))
        .or_else(|| Some(entity.id.clone()))
        .filter(|id| !id.trim().is_empty())
}

fn entity_playlist_id(entity: &SourceEntityRef) -> Option<String> {
    scalar_string(&entity.platform_ids, "playlistId")
        .or_else(|| json_object_string(&entity.raw_info, "playlistId"))
        .or_else(|| json_object_string(&entity.raw_info, "browseId"))
        .or_else(|| Some(entity.id.clone()))
        .filter(|id| !id.trim().is_empty())
}

fn music_info_video_id(music_info: &JsonValue) -> Option<String> {
    ["videoId", "id"]
        .into_iter()
        .find_map(|key| json_object_string(music_info, key))
        .filter(|id| valid_video_id(id))
}

fn valid_video_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn renderer_video_id(item: &JsonValue) -> Option<String> {
    string_at(
        item,
        &[
            "overlay",
            "musicItemThumbnailOverlayRenderer",
            "content",
            "musicPlayButtonRenderer",
            "playNavigationEndpoint",
            "watchEndpoint",
            "videoId",
        ],
    )
    .or_else(|| find_string_for_key(item, "videoId"))
    .filter(|id| valid_video_id(id))
}

fn renderer_browse_id(item: &JsonValue) -> Option<String> {
    string_at(item, &["navigationEndpoint", "browseEndpoint", "browseId"]).or_else(|| {
        flex_runs(item, 0)
            .iter()
            .find_map(|run| string_at(run, &["navigationEndpoint", "browseEndpoint", "browseId"]))
    })
}

fn flex_runs(item: &JsonValue, index: usize) -> &[JsonValue] {
    item.get("flexColumns")
        .and_then(JsonValue::as_array)
        .and_then(|columns| columns.get(index))
        .and_then(|column| column.get("musicResponsiveListItemFlexColumnRenderer"))
        .and_then(|renderer| renderer.get("text"))
        .and_then(|text| text.get("runs"))
        .and_then(JsonValue::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn flex_text(item: &JsonValue, index: usize) -> Option<String> {
    joined_run_text(flex_runs(item, index))
}

fn fixed_text(item: &JsonValue, index: usize) -> Option<String> {
    item.get("fixedColumns")
        .and_then(JsonValue::as_array)
        .and_then(|columns| columns.get(index))
        .and_then(|column| column.get("musicResponsiveListItemFixedColumnRenderer"))
        .and_then(|renderer| renderer.get("text"))
        .and_then(value_text)
}

fn text_at(value: &JsonValue, path: &[&str]) -> Option<String> {
    let target = value_at(value, path)?;
    value_text(target)
}

fn value_text(value: &JsonValue) -> Option<String> {
    value
        .get("simpleText")
        .and_then(JsonValue::as_str)
        .map(str::to_owned)
        .or_else(|| {
            value
                .get("runs")
                .and_then(JsonValue::as_array)
                .and_then(|runs| joined_run_text(runs))
        })
        .or_else(|| value.as_str().map(str::to_owned))
        .filter(|text| !text.trim().is_empty())
}

fn joined_run_text(runs: &[JsonValue]) -> Option<String> {
    let text = runs
        .iter()
        .filter_map(|run| run.get("text").and_then(JsonValue::as_str))
        .collect::<String>();
    (!text.trim().is_empty()).then_some(text)
}

fn runs_at<'a>(value: &'a JsonValue, path: &[&str]) -> &'a [JsonValue] {
    value_at(value, path)
        .and_then(|value| value.get("runs"))
        .and_then(JsonValue::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn value_at<'a>(value: &'a JsonValue, path: &[&str]) -> Option<&'a JsonValue> {
    path.iter()
        .try_fold(value, |current, key| current.get(*key))
}

fn string_at(value: &JsonValue, path: &[&str]) -> Option<String> {
    value_at(value, path)
        .and_then(JsonValue::as_str)
        .map(str::to_owned)
}

fn run_page_type(run: &JsonValue) -> Option<&str> {
    value_at(
        run,
        &[
            "navigationEndpoint",
            "browseEndpoint",
            "browseEndpointContextSupportedConfigs",
            "browseEndpointContextMusicConfig",
            "pageType",
        ],
    )
    .and_then(JsonValue::as_str)
}

fn run_names_for_page_types(runs: &[JsonValue], page_types: &[&str]) -> Vec<String> {
    let mut names = Vec::new();
    for run in runs {
        if run_page_type(run).is_some_and(|page_type| page_types.contains(&page_type)) {
            if let Some(name) = run.get("text").and_then(JsonValue::as_str) {
                push_unique(&mut names, name.to_owned());
            }
        }
    }
    names
}

fn run_name_for_page_type(runs: &[JsonValue], page_type: &str) -> Option<String> {
    runs.iter().find_map(|run| {
        (run_page_type(run) == Some(page_type))
            .then(|| {
                run.get("text")
                    .and_then(JsonValue::as_str)
                    .map(str::to_owned)
            })
            .flatten()
    })
}

fn run_browse_id_for_page_type(runs: &[JsonValue], page_type: &str) -> Option<String> {
    runs.iter().find_map(|run| {
        (run_page_type(run) == Some(page_type))
            .then(|| string_at(run, &["navigationEndpoint", "browseEndpoint", "browseId"]))
            .flatten()
    })
}

fn fallback_metadata_name(runs: &[JsonValue]) -> Option<String> {
    metadata_texts(runs).into_iter().find(|text| {
        !matches!(
            text.as_str(),
            "Song" | "Video" | "Album" | "Single" | "EP" | "Playlist"
        ) && parse_duration(text).is_none()
            && parse_year(text).is_none()
            && text != "•"
    })
}

fn metadata_texts(runs: &[JsonValue]) -> Vec<String> {
    runs.iter()
        .filter_map(|run| run.get("text").and_then(JsonValue::as_str))
        .map(str::trim)
        .filter(|text| !text.is_empty() && *text != "•")
        .map(str::to_owned)
        .collect()
}

fn best_thumbnail_url(value: &JsonValue) -> Option<String> {
    let mut candidates = Vec::<(u64, String)>::new();
    collect_thumbnail_candidates(value, &mut candidates);
    candidates
        .into_iter()
        .max_by_key(|(area, _)| *area)
        .map(|(_, url)| url)
}

fn collect_thumbnail_candidates(value: &JsonValue, output: &mut Vec<(u64, String)>) {
    match value {
        JsonValue::Object(object) => {
            if let Some(thumbnails) = object.get("thumbnails").and_then(JsonValue::as_array) {
                for thumbnail in thumbnails {
                    if let Some(url) = thumbnail.get("url").and_then(JsonValue::as_str) {
                        let width = thumbnail
                            .get("width")
                            .and_then(JsonValue::as_u64)
                            .unwrap_or_default();
                        let height = thumbnail
                            .get("height")
                            .and_then(JsonValue::as_u64)
                            .unwrap_or_default();
                        output.push((width.saturating_mul(height), url.to_owned()));
                    }
                }
            }
            object
                .values()
                .for_each(|child| collect_thumbnail_candidates(child, output));
        }
        JsonValue::Array(values) => values
            .iter()
            .for_each(|child| collect_thumbnail_candidates(child, output)),
        _ => {}
    }
}

fn collect_values_for_key(value: &JsonValue, key: &str, output: &mut Vec<JsonValue>) {
    match value {
        JsonValue::Object(object) => {
            if let Some(found) = object.get(key) {
                output.push(found.clone());
            }
            object
                .values()
                .for_each(|child| collect_values_for_key(child, key, output));
        }
        JsonValue::Array(values) => values
            .iter()
            .for_each(|child| collect_values_for_key(child, key, output)),
        _ => {}
    }
}

fn find_value_for_key<'a>(value: &'a JsonValue, key: &str) -> Option<&'a JsonValue> {
    match value {
        JsonValue::Object(object) => object.get(key).or_else(|| {
            object
                .values()
                .find_map(|child| find_value_for_key(child, key))
        }),
        JsonValue::Array(values) => values
            .iter()
            .find_map(|child| find_value_for_key(child, key)),
        _ => None,
    }
}

fn find_string_for_key(value: &JsonValue, key: &str) -> Option<String> {
    find_value_for_key(value, key)
        .and_then(JsonValue::as_str)
        .map(str::to_owned)
}

fn find_continuation_token(value: &JsonValue) -> Option<String> {
    match value {
        JsonValue::Object(object) => {
            if let Some(token) = object
                .get("nextContinuationData")
                .and_then(|next| next.get("continuation"))
                .and_then(JsonValue::as_str)
            {
                return Some(token.to_owned());
            }
            if let Some(token) = object
                .get("continuationCommand")
                .and_then(|command| command.get("token"))
                .and_then(JsonValue::as_str)
            {
                return Some(token.to_owned());
            }
            object.values().find_map(find_continuation_token)
        }
        JsonValue::Array(values) => values.iter().find_map(find_continuation_token),
        _ => None,
    }
}

fn find_browse_id_for_page_type(value: &JsonValue, target: &str) -> Option<String> {
    match value {
        JsonValue::Object(object) => {
            if let Some(endpoint) = object.get("browseEndpoint") {
                let page_type = string_at(
                    endpoint,
                    &[
                        "browseEndpointContextSupportedConfigs",
                        "browseEndpointContextMusicConfig",
                        "pageType",
                    ],
                );
                if page_type.as_deref() == Some(target) {
                    return endpoint
                        .get("browseId")
                        .and_then(JsonValue::as_str)
                        .map(str::to_owned);
                }
            }
            object
                .values()
                .find_map(|child| find_browse_id_for_page_type(child, target))
        }
        JsonValue::Array(values) => values
            .iter()
            .find_map(|child| find_browse_id_for_page_type(child, target)),
        _ => None,
    }
}

fn parse_duration(value: &str) -> Option<u64> {
    let parts = value
        .trim()
        .split(':')
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    parts.into_iter().try_fold(0_u64, |total, part| {
        total.checked_mul(60)?.checked_add(part)
    })
}

fn parse_year(value: &str) -> Option<u32> {
    let year = value.trim().parse::<u32>().ok()?;
    (1900..=2200).contains(&year).then_some(year)
}

fn parse_count(value: &str) -> Option<u64> {
    let digits = value
        .chars()
        .filter(char::is_ascii_digit)
        .collect::<String>();
    (!digits.is_empty()).then(|| digits.parse().ok()).flatten()
}

fn paginate<T>(items: Vec<T>, page: u64, page_size: u64) -> (Vec<T>, bool) {
    let start = page.saturating_sub(1).saturating_mul(page_size) as usize;
    let total = items.len();
    let list = items
        .into_iter()
        .skip(start)
        .take(page_size as usize)
        .collect::<Vec<_>>();
    (list, start.saturating_add(page_size as usize) >= total)
}

fn browse_platform_ids(id: &str) -> BTreeMap<String, JsonScalar> {
    BTreeMap::from([("browseId".to_owned(), JsonScalar::String(id.to_owned()))])
}

fn scalar_string(ids: &BTreeMap<String, JsonScalar>, key: &str) -> Option<String> {
    match ids.get(key) {
        Some(JsonScalar::String(value)) => Some(value.clone()),
        _ => None,
    }
}

fn json_object_string(value: &JsonValue, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(JsonValue::as_str)
        .map(str::to_owned)
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}

fn empty_lyrics() -> LyricResponse {
    LyricResponse {
        lyric: None,
        tlyric: None,
        rlyric: None,
        lxlyric: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_runtime::{DefaultSourceHost, SourceRuntime};
    use std::sync::Arc;
    use std::time::Duration;

    fn browse_endpoint(id: &str, page_type: &str) -> JsonValue {
        json!({
            "browseEndpoint": {
                "browseId": id,
                "browseEndpointContextSupportedConfigs": {
                    "browseEndpointContextMusicConfig": { "pageType": page_type }
                }
            }
        })
    }

    fn song_renderer() -> JsonValue {
        json!({
            "flexColumns": [
                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [
                    {"text": "Wonderwall"}
                ]}}},
                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [
                    {"text": "Song"}, {"text": " • "},
                    {"text": "Oasis", "navigationEndpoint": browse_endpoint("UC-oasis", "MUSIC_PAGE_TYPE_ARTIST")},
                    {"text": " • "},
                    {"text": "Morning Glory", "navigationEndpoint": browse_endpoint("MPRE-album", "MUSIC_PAGE_TYPE_ALBUM")},
                    {"text": " • "}, {"text": "4:19"}
                ]}}}
            ],
            "fixedColumns": [{"musicResponsiveListItemFixedColumnRenderer": {
                "text": {"runs": [{"text": "4:19"}]}
            }}],
            "overlay": {"musicItemThumbnailOverlayRenderer": {"content": {
                "musicPlayButtonRenderer": {"playNavigationEndpoint": {"watchEndpoint": {
                    "videoId": "ZrOKjDZOtkA"
                }}}
            }}},
            "thumbnail": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": [
                {"url": "https://i.ytimg.com/small.jpg", "width": 60, "height": 60},
                {"url": "https://i.ytimg.com/large.jpg", "width": 544, "height": 544}
            ]}}}
        })
    }

    #[test]
    fn bootstrap_config_is_extracted_without_hard_coded_credentials() {
        let config = parse_web_client_config(
            r#"ytcfg.set({"INNERTUBE_API_KEY":"key","INNERTUBE_CLIENT_VERSION":"1.20260725.00.00","VISITOR_DATA":"visitor%3D"});"#,
        )
        .expect("config should parse");

        assert_eq!(config.api_key, "key");
        assert_eq!(config.client_version, "1.20260725.00.00");
        assert_eq!(config.visitor_data.as_deref(), Some("visitor%3D"));
    }

    #[test]
    fn song_renderer_preserves_youtube_track_identity() {
        let track = track_from_renderer(&song_renderer(), None, None, None)
            .expect("song renderer should parse");

        assert_eq!(track.id, "ZrOKjDZOtkA");
        assert_eq!(track.artist, "Oasis");
        assert_eq!(track.album.as_deref(), Some("Morning Glory"));
        assert_eq!(track.duration_seconds, Some(259));
        assert_eq!(
            track.cover_url.as_deref(),
            Some("https://i.ytimg.com/large.jpg")
        );
        assert_eq!(
            track.platform_ids.get("videoId"),
            Some(&JsonScalar::String("ZrOKjDZOtkA".to_owned()))
        );
        assert_eq!(track.raw_info["videoId"], "ZrOKjDZOtkA");
    }

    #[test]
    fn lyrics_tab_is_found_by_page_type_instead_of_tab_position() {
        let response = json!({
            "tabs": [
                {"tabRenderer": {"endpoint": browse_endpoint("related", "MUSIC_PAGE_TYPE_TRACK_RELATED")}},
                {"tabRenderer": {"endpoint": browse_endpoint("MPLYt-lyrics", "MUSIC_PAGE_TYPE_TRACK_LYRICS")}}
            ]
        });

        assert_eq!(
            find_browse_id_for_page_type(&response, "MUSIC_PAGE_TYPE_TRACK_LYRICS").as_deref(),
            Some("MPLYt-lyrics")
        );
    }

    #[test]
    fn catalog_provider_does_not_claim_playback() {
        let provider = YoutubeMusicSourceProvider::new(
            YOUTUBE_MUSIC_PROVIDER_ID.to_owned(),
            BTreeSet::from([SourceCapability::NetworkAny]),
        );
        let runtime = SourceRuntime::new();
        let report = runtime
            .initialize_provider(&provider)
            .expect("provider should initialize");
        let actions = &report.sources[YOUTUBE_MUSIC_SOURCE_ID].actions;

        assert!(actions.contains(&SourceAction::MusicSearch));
        assert!(actions.contains(&SourceAction::Lyric));
        assert!(!actions.contains(&SourceAction::MusicUrl));
    }

    #[test]
    #[ignore = "live YouTube Music contract test"]
    fn live_catalog_search_returns_standard_video_ids() {
        let host = Arc::new(DefaultSourceHost::new(
            Duration::from_secs(12),
            4 * 1024 * 1024,
        ));
        let runtime = SourceRuntime::with_host(host, []);
        let provider = YoutubeMusicSourceProvider::new(
            YOUTUBE_MUSIC_PROVIDER_ID.to_owned(),
            BTreeSet::from([SourceCapability::NetworkAny]),
        );
        runtime
            .replace_provider_granted_capabilities(
                YOUTUBE_MUSIC_PROVIDER_ID,
                [SourceCapability::NetworkAny],
            )
            .expect("network grant should install");
        runtime
            .initialize_provider(&provider)
            .expect("provider should initialize");

        let outcome = runtime
            .dispatch_request(
                &provider,
                SourceRequest::MusicSearch {
                    source: YOUTUBE_MUSIC_SOURCE_ID.to_owned(),
                    keyword: "Oasis Wonderwall".to_owned(),
                    page: 1,
                    page_size: 5,
                },
            )
            .expect("public catalog search should succeed");
        let SourceResponse::MusicSearch(response) = outcome.response else {
            panic!("search returned the wrong response variant");
        };

        assert!(!response.list.is_empty());
        assert!(response.list.iter().all(|track| valid_video_id(&track.id)));
    }

    #[test]
    #[ignore = "live YouTube Music detail contract test"]
    fn live_catalog_detail_routes_return_typed_public_data() {
        let host = Arc::new(DefaultSourceHost::new(
            Duration::from_secs(15),
            4 * 1024 * 1024,
        ));
        let runtime = SourceRuntime::with_host(host, []);
        let provider = YoutubeMusicSourceProvider::new(
            YOUTUBE_MUSIC_PROVIDER_ID.to_owned(),
            BTreeSet::from([SourceCapability::NetworkAny]),
        );
        runtime
            .replace_provider_granted_capabilities(
                YOUTUBE_MUSIC_PROVIDER_ID,
                [SourceCapability::NetworkAny],
            )
            .expect("network grant should install");
        runtime
            .initialize_provider(&provider)
            .expect("provider should initialize");

        let suggestions = runtime
            .dispatch_request(
                &provider,
                SourceRequest::SearchSuggestions {
                    source: YOUTUBE_MUSIC_SOURCE_ID.to_owned(),
                    keyword: "Oasis Wonder".to_owned(),
                    limit: 5,
                },
            )
            .expect("suggestions should load");
        assert!(matches!(
            suggestions.response,
            SourceResponse::SearchSuggestions(SourceSuggestionsResponse { ref list })
                if !list.is_empty()
        ));

        let artist_search = runtime
            .dispatch_request(
                &provider,
                SourceRequest::ArtistSearch {
                    source: YOUTUBE_MUSIC_SOURCE_ID.to_owned(),
                    keyword: "Oasis".to_owned(),
                    page: 1,
                    page_size: 5,
                },
            )
            .expect("artist search should load");
        let SourceResponse::ArtistSearch(artist_search) = artist_search.response else {
            panic!("artist search returned the wrong variant");
        };
        let artist = artist_search
            .list
            .into_iter()
            .next()
            .expect("artist search should return a result");
        let artist_ref = SourceEntityRef {
            id: artist.id,
            platform_ids: artist.platform_ids,
            raw_info: artist.raw_info,
        };
        let top_tracks = runtime
            .dispatch_request(
                &provider,
                SourceRequest::ArtistTopTracks {
                    source: YOUTUBE_MUSIC_SOURCE_ID.to_owned(),
                    artist: artist_ref.clone(),
                    limit: 5,
                },
            )
            .expect("artist tracks should load");
        let SourceResponse::ArtistTopTracks(top_tracks) = top_tracks.response else {
            panic!("artist tracks returned the wrong variant");
        };
        assert!(!top_tracks.list.is_empty());
        let video_id = top_tracks.list[0].id.clone();

        let albums = runtime
            .dispatch_request(
                &provider,
                SourceRequest::ArtistAlbums {
                    source: YOUTUBE_MUSIC_SOURCE_ID.to_owned(),
                    artist: artist_ref.clone(),
                    page: 1,
                    page_size: 5,
                },
            )
            .expect("artist albums should load");
        assert!(matches!(
            albums.response,
            SourceResponse::ArtistAlbums(SourceAlbumSearchResponse { ref list, .. })
                if !list.is_empty()
        ));
        let biography = runtime
            .dispatch_request(
                &provider,
                SourceRequest::ArtistBiography {
                    source: YOUTUBE_MUSIC_SOURCE_ID.to_owned(),
                    artist: artist_ref,
                },
            )
            .expect("artist biography should load");
        assert!(matches!(
            biography.response,
            SourceResponse::ArtistBiography(_)
        ));

        let album_search = runtime
            .dispatch_request(
                &provider,
                SourceRequest::AlbumSearch {
                    source: YOUTUBE_MUSIC_SOURCE_ID.to_owned(),
                    keyword: "Oasis Morning Glory".to_owned(),
                    page: 1,
                    page_size: 5,
                },
            )
            .expect("album search should load");
        let SourceResponse::AlbumSearch(album_search) = album_search.response else {
            panic!("album search returned the wrong variant");
        };
        let album = album_search
            .list
            .into_iter()
            .next()
            .expect("album search should return a result");
        let album_tracks = runtime
            .dispatch_request(
                &provider,
                SourceRequest::AlbumRead {
                    source: YOUTUBE_MUSIC_SOURCE_ID.to_owned(),
                    album: SourceEntityRef {
                        id: album.id,
                        platform_ids: album.platform_ids,
                        raw_info: album.raw_info,
                    },
                    page: 1,
                    page_size: 5,
                },
            )
            .expect("album tracks should load");
        assert!(matches!(
            album_tracks.response,
            SourceResponse::AlbumRead(SourceSearchResponse { ref list, .. })
                if !list.is_empty()
        ));

        let playlist_search = runtime
            .dispatch_request(
                &provider,
                SourceRequest::PlaylistSearch {
                    source: YOUTUBE_MUSIC_SOURCE_ID.to_owned(),
                    keyword: "Oasis essentials".to_owned(),
                    page: 1,
                    page_size: 5,
                },
            )
            .expect("playlist search should load");
        let SourceResponse::PlaylistSearch(playlist_search) = playlist_search.response else {
            panic!("playlist search returned the wrong variant");
        };
        let playlist = playlist_search
            .list
            .into_iter()
            .next()
            .expect("playlist search should return a result");
        let playlist_tracks = runtime
            .dispatch_request(
                &provider,
                SourceRequest::PlaylistReadPublic {
                    source: YOUTUBE_MUSIC_SOURCE_ID.to_owned(),
                    playlist: SourceEntityRef {
                        id: playlist.id,
                        platform_ids: playlist.platform_ids,
                        raw_info: playlist.raw_info,
                    },
                    page: 1,
                    page_size: 5,
                },
            )
            .expect("playlist tracks should load");
        assert!(matches!(
            playlist_tracks.response,
            SourceResponse::PlaylistReadPublic(SourceSearchResponse { ref list, .. })
                if !list.is_empty()
        ));

        let lyrics = runtime
            .dispatch_request(
                &provider,
                SourceRequest::Lyric {
                    source: YOUTUBE_MUSIC_SOURCE_ID.to_owned(),
                    music_info: json!({ "videoId": video_id }),
                },
            )
            .expect("lyrics request should complete");
        assert!(matches!(lyrics.response, SourceResponse::Lyric(_)));
    }
}
