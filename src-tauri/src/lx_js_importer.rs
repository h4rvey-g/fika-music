use crate::source_runtime::{
    lx_music_source, SourceAction, SourceCapability, SourceHttpRequest, SourceInfo, SourceProvider,
    SourceQuality, SourceRequest, SourceResponse, SourceRuntimeContext, SourceRuntimeError,
    SourceSearchResponse, SourceSearchResult, LX_SOURCE_KG, LX_SOURCE_KIND_MUSIC, LX_SOURCE_KW,
    LX_SOURCE_LOCAL, LX_SOURCE_MG, LX_SOURCE_TX, LX_SOURCE_WY,
};
use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

const LX_SOURCE_KEYS: &[&str] = &[
    LX_SOURCE_KG,
    LX_SOURCE_TX,
    LX_SOURCE_WY,
    LX_SOURCE_KW,
    LX_SOURCE_MG,
    LX_SOURCE_LOCAL,
];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LxJsImportReport {
    pub file_name: String,
    pub metadata: LxJsMetadata,
    pub parse: JsParseReport,
    pub contract: LxJsContractReport,
    pub endpoint: LxJsEndpointReport,
    pub obfuscation: LxJsObfuscationReport,
    pub deobfuscation: LxJsDeobfuscationReport,
    pub manifest: ImportedLxManifest,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LxJsMetadata {
    pub name: Option<String>,
    pub description: Option<String>,
    pub version: Option<String>,
    pub author: Option<String>,
    pub homepage: Option<String>,
    pub update_url: Option<String>,
    pub raw_tags: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JsParseReport {
    pub parsed_with_oxc: bool,
    pub top_level_statement_count: usize,
    pub comment_count: usize,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LxJsContractReport {
    pub uses_global_lx: bool,
    pub uses_event_names: bool,
    pub uses_lx_request: bool,
    pub registers_request_handler: bool,
    pub sends_any_lx_event: bool,
    pub sends_inited_event_literal: bool,
    pub sends_update_alert_event_literal: bool,
    pub declared_actions: Vec<SourceAction>,
    pub declared_qualities: Vec<SourceQuality>,
    pub declared_sources: BTreeMap<String, ImportedLxSourceInfo>,
    pub requires_deobfuscation_for_full_manifest: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LxJsEndpointReport {
    pub urls: Vec<String>,
    pub domains: Vec<String>,
    pub templates: Vec<LxUrlTemplate>,
    pub template_count: usize,
    pub decoded_url_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LxUrlTemplate {
    pub family: String,
    pub source_id: String,
    pub url: String,
    pub domain: Option<String>,
    pub has_track_id_placeholder: bool,
    pub has_quality_placeholder: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImportedLxSourceInfo {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub actions: Vec<SourceAction>,
    pub qualities: Vec<SourceQuality>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImportedLxManifest {
    pub provider_id: String,
    pub display_name: String,
    pub version: Option<String>,
    pub author: Option<String>,
    pub homepage: Option<String>,
    pub update_url: Option<String>,
    pub sources: BTreeMap<String, ImportedLxSourceInfo>,
    pub requires_rust_port: bool,
    pub warnings: Vec<String>,
}

impl ImportedLxManifest {
    pub fn to_source_catalog(&self) -> BTreeMap<String, SourceInfo> {
        self.sources
            .iter()
            .map(|(source_id, source)| {
                (
                    source_id.clone(),
                    lx_music_source(
                        source.id.clone(),
                        source.name.clone(),
                        source.actions.clone(),
                        source.qualities.clone(),
                    ),
                )
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct ImportedLxManifestProvider {
    manifest: ImportedLxManifest,
}

impl ImportedLxManifestProvider {
    pub fn new(manifest: ImportedLxManifest) -> Self {
        Self { manifest }
    }

    pub fn manifest(&self) -> &ImportedLxManifest {
        &self.manifest
    }
}

impl SourceProvider for ImportedLxManifestProvider {
    fn id(&self) -> &str {
        &self.manifest.provider_id
    }

    fn required_capabilities(&self) -> BTreeSet<SourceCapability> {
        BTreeSet::new()
    }

    fn initialize(
        &self,
        context: &mut SourceRuntimeContext,
    ) -> Result<BTreeMap<String, SourceInfo>, SourceRuntimeError> {
        context.info(format!(
            "loaded imported LX manifest for {}; Rust provider port still required",
            self.manifest.display_name
        ));
        for warning in &self.manifest.warnings {
            context.warn(warning.clone());
        }
        Ok(self.manifest.to_source_catalog())
    }

    fn handle_request(
        &self,
        context: &mut SourceRuntimeContext,
        request: SourceRequest,
    ) -> Result<SourceResponse, SourceRuntimeError> {
        context.warn(format!(
            "imported LX manifest {} has no Rust port for {:?}",
            self.manifest.display_name,
            request.action()
        ));
        Err(context
            .provider_error("imported LX JS manifests are catalog-only until ported to Rust"))
    }
}

const QISHUI_SOURCE_ID: &str = "qsvip";
const QISHUI_SOURCE_NAME: &str = "汽水VIP";
const QISHUI_API_HTTPS: &str = "https://api.vsaa.cn/api/music.qishui.vip";
const QISHUI_API_HTTP: &str = "http://api.vsaa.cn/api/music.qishui.vip";
const QISHUI_PROXY_API: &str = "https://proxy.qishui.vsaa.cn/qishui/proxy";

#[derive(Debug, Clone)]
pub struct QishuiRustProvider {
    api_https_url: String,
    api_http_url: String,
    proxy_url: String,
}

impl Default for QishuiRustProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl QishuiRustProvider {
    pub fn new() -> Self {
        Self::with_endpoints(QISHUI_API_HTTPS, QISHUI_API_HTTP, QISHUI_PROXY_API)
    }

    pub fn with_endpoints(
        api_https_url: impl Into<String>,
        api_http_url: impl Into<String>,
        proxy_url: impl Into<String>,
    ) -> Self {
        Self {
            api_https_url: api_https_url.into(),
            api_http_url: api_http_url.into(),
            proxy_url: proxy_url.into(),
        }
    }

    fn search(
        &self,
        context: &mut SourceRuntimeContext,
        keyword: &str,
        page: u64,
        page_size: u64,
    ) -> Result<SourceSearchResponse, SourceRuntimeError> {
        let response = self.get_with_fallback(
            context,
            &[
                ("act", "search".to_owned()),
                ("keywords", keyword.trim().to_owned()),
                ("page", page.to_string()),
                ("pagesize", page_size.to_string()),
                ("type", "music".to_owned()),
            ],
        )?;
        let list_values = response
            .pointer("/data/lists")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let total = response.pointer("/data/total").and_then(json_u64);
        let list = list_values.iter().map(normalize_qishui_song_info).collect();

        Ok(SourceSearchResponse {
            is_end: list_values.len() < usize::try_from(page_size).unwrap_or(usize::MAX),
            total: total.or(Some(list_values.len() as u64)),
            list,
        })
    }

    fn resolve_music_url(
        &self,
        context: &mut SourceRuntimeContext,
        music_info: &serde_json::Value,
        quality: SourceQuality,
    ) -> Result<String, SourceRuntimeError> {
        let Some(song_id) = qishui_song_id(music_info) else {
            return Err(context.provider_error("汽水VIP缺少歌曲ID"));
        };
        let quality = normalize_qishui_quality(quality);

        let response = self.get_with_fallback(
            context,
            &[
                ("act", "song".to_owned()),
                ("id", song_id),
                ("quality", quality.to_owned()),
            ],
        )?;
        let Some(data) = qishui_first_data(&response) else {
            return Err(context.provider_error("汽水VIP未返回歌曲数据"));
        };
        let Some(url) = data.get("url").and_then(serde_json::Value::as_str) else {
            return Err(context.provider_error("汽水VIP未返回可用URL"));
        };
        if let Some(ekey) = data.get("ekey").and_then(serde_json::Value::as_str) {
            if !ekey.is_empty() {
                return self.resolve_proxy_url(context, url, ekey, data);
            }
        }
        Ok(url.to_owned())
    }

    fn fetch_lyric(
        &self,
        context: &mut SourceRuntimeContext,
        music_info: &serde_json::Value,
    ) -> Result<String, SourceRuntimeError> {
        let Some(song_id) = qishui_song_id(music_info) else {
            return Ok(String::new());
        };
        let response =
            self.get_with_fallback(context, &[("act", "song".to_owned()), ("id", song_id)])?;
        Ok(qishui_first_data(&response)
            .and_then(|data| data.get("lyric"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned())
    }

    fn get_with_fallback(
        &self,
        context: &mut SourceRuntimeContext,
        params: &[(&str, String)],
    ) -> Result<serde_json::Value, SourceRuntimeError> {
        let mut last_error = None;
        for url in [&self.api_https_url, &self.api_http_url] {
            match self.get_json(context, url, params) {
                Ok(value) => return Ok(value),
                Err(error @ SourceRuntimeError::Cancelled { .. }) => return Err(error),
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or_else(|| context.provider_error("汽水VIP请求失败")))
    }

    fn get_json(
        &self,
        context: &mut SourceRuntimeContext,
        url: &str,
        params: &[(&str, String)],
    ) -> Result<serde_json::Value, SourceRuntimeError> {
        let url = append_query_params(url, params);
        let response =
            context.http_request(SourceHttpRequest::get(url), "request qsvip endpoint")?;
        if !response.is_success() {
            return Err(context.provider_error(format!("汽水VIP HTTP {}", response.status)));
        }
        response
            .json::<serde_json::Value>()
            .map_err(|error| context.provider_error(format!("汽水VIP响应解析失败: {error}")))
    }

    fn resolve_proxy_url(
        &self,
        context: &mut SourceRuntimeContext,
        url: &str,
        ekey: &str,
        data: &serde_json::Value,
    ) -> Result<String, SourceRuntimeError> {
        let filename = data
            .get("filename")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("KMusic");
        let ext = data
            .get("fileExtension")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("aac");
        let response = context.http_request(
            SourceHttpRequest::post_json(
                &self.proxy_url,
                serde_json::json!({
                "url": url,
                "key": ekey,
                "filename": filename,
                "ext": ext,
                }),
            ),
            "request qsvip proxy",
        )?;
        if !response.is_success() {
            return Err(context.provider_error(format!("汽水VIP代理 HTTP {}", response.status)));
        }
        let body = response
            .json::<serde_json::Value>()
            .map_err(|error| context.provider_error(format!("汽水VIP代理响应解析失败: {error}")))?;
        body.get("url")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| context.provider_error("汽水VIP代理未返回可用URL"))
    }
}

impl SourceProvider for QishuiRustProvider {
    fn id(&self) -> &str {
        QISHUI_SOURCE_ID
    }

    fn required_capabilities(&self) -> BTreeSet<SourceCapability> {
        BTreeSet::from([SourceCapability::NetworkAny])
    }

    fn initialize(
        &self,
        context: &mut SourceRuntimeContext,
    ) -> Result<BTreeMap<String, SourceInfo>, SourceRuntimeError> {
        context.info("initialized Rust qsvip search provider");
        Ok(BTreeMap::from([(
            QISHUI_SOURCE_ID.to_owned(),
            lx_music_source(
                QISHUI_SOURCE_ID,
                QISHUI_SOURCE_NAME,
                vec![
                    SourceAction::MusicSearch,
                    SourceAction::MusicUrl,
                    SourceAction::Lyric,
                ],
                vec![
                    SourceQuality::K128,
                    SourceQuality::K320,
                    SourceQuality::Flac,
                    SourceQuality::Flac24Bit,
                ],
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
            } => {
                context.require_capability(SourceCapability::NetworkAny, "search qsvip music")?;
                let response = self.search(context, &keyword, page, page_size)?;
                context.info(format!(
                    "qsvip search returned {} result(s)",
                    response.list.len()
                ));
                Ok(SourceResponse::MusicSearch(response))
            }
            SourceRequest::MusicUrl {
                music_info,
                quality,
                ..
            } => {
                context
                    .require_capability(SourceCapability::NetworkAny, "resolve qsvip musicUrl")?;
                let url = self.resolve_music_url(context, &music_info, quality)?;
                context.info("resolved qsvip musicUrl");
                Ok(SourceResponse::MusicUrl(url))
            }
            SourceRequest::Lyric { music_info, .. } => {
                context.require_capability(SourceCapability::NetworkAny, "fetch qsvip lyric")?;
                let lyric = self.fetch_lyric(context, &music_info)?;
                Ok(SourceResponse::Lyric(
                    crate::source_runtime::LyricResponse {
                        lyric: Some(lyric),
                        tlyric: None,
                        rlyric: None,
                        lxlyric: None,
                    },
                ))
            }
            SourceRequest::Pic { source, .. } => {
                Err(context.unsupported_action(source, SourceAction::Pic))
            }
            request => Err(context.unsupported_action(request.source(), request.action())),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ImportedLxTemplateProvider {
    provider_id: String,
    manifest: ImportedLxManifest,
    family: String,
    templates_by_source: BTreeMap<String, LxUrlTemplate>,
}

impl ImportedLxTemplateProvider {
    pub fn from_report(report: &LxJsImportReport, family: &str) -> Result<Self, LxJsImportError> {
        Ok(Self {
            provider_id: format!("{}-{family}-template-preview", report.manifest.provider_id),
            manifest: report.manifest.clone(),
            family: family.to_owned(),
            templates_by_source: report
                .endpoint
                .templates
                .iter()
                .filter(|template| template.family == family)
                .map(|template| (template.source_id.clone(), template.clone()))
                .collect(),
        })
    }

    fn template_for_source(&self, source: &str) -> Option<&LxUrlTemplate> {
        self.templates_by_source.get(source)
    }

    fn resolved_template_url(
        &self,
        context: &mut SourceRuntimeContext,
        endpoint_url: &str,
    ) -> Result<String, SourceRuntimeError> {
        resolve_template_endpoint(context, endpoint_url)
    }
}

impl SourceProvider for ImportedLxTemplateProvider {
    fn id(&self) -> &str {
        &self.provider_id
    }

    fn required_capabilities(&self) -> BTreeSet<SourceCapability> {
        BTreeSet::from([SourceCapability::NetworkAny])
    }

    fn initialize(
        &self,
        context: &mut SourceRuntimeContext,
    ) -> Result<BTreeMap<String, SourceInfo>, SourceRuntimeError> {
        context.warn(format!(
            "loaded {} URL template preview for {}; URLs are candidates until the Rust port validates responses",
            self.family, self.manifest.display_name
        ));
        Ok(self.manifest.to_source_catalog())
    }

    fn handle_request(
        &self,
        context: &mut SourceRuntimeContext,
        request: SourceRequest,
    ) -> Result<SourceResponse, SourceRuntimeError> {
        let SourceRequest::MusicUrl {
            source,
            music_info,
            quality,
        } = request
        else {
            return Err(context.unsupported_action(request.source().to_owned(), request.action()));
        };
        context.require_capability(SourceCapability::NetworkAny, "build imported URL template")?;

        let Some(template) = self.template_for_source(&source) else {
            return Err(context.provider_error(format!(
                "no {} URL template candidate for source {}",
                self.family, source
            )));
        };
        let Some(track_id) = extract_track_id(&music_info) else {
            return Err(context.provider_error("musicUrl request is missing a track id"));
        };

        let level = quality_to_template_level(quality);
        let url = template
            .url
            .replace("{id}", &encode_component(&track_id))
            .replace("{level}", &encode_component(level));

        let resolved_url = self.resolved_template_url(context, &url)?;

        context.info(format!(
            "resolved playable musicUrl via imported {} Rust template provider",
            self.family
        ));
        Ok(SourceResponse::MusicUrl(resolved_url))
    }
}

/*
 * The network client belongs to SourceRuntime's host service. Keeping endpoint
 * resolution here as a pure response normalizer prevents imported sources from
 * acquiring a second, unreviewed HTTP boundary.
 */
fn resolve_template_endpoint(
    context: &mut SourceRuntimeContext,
    endpoint_url: &str,
) -> Result<String, SourceRuntimeError> {
    let response = context.http_request(
        SourceHttpRequest::get(endpoint_url),
        "request imported template endpoint",
    )?;

    if !response.is_success() {
        return Err(context.provider_error(format!(
            "template endpoint {endpoint_url} returned HTTP {}",
            response.status
        )));
    }

    let final_url = response.final_url.clone();
    let content_type = response
        .content_type
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if content_type.starts_with("audio/") || content_type.contains("octet-stream") {
        return Ok(final_url);
    }

    let body = response
        .text()
        .map_err(|error| context.provider_error(error.to_string()))?;
    let body = body.trim();
    if is_http_url(body) {
        return Ok(body.trim_matches('"').to_owned());
    }
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(url) = find_url_in_json(&json) {
            return Ok(url);
        }
    }

    Err(context.provider_error("template endpoint did not return a playable URL"))
}

fn find_url_in_json(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) if is_http_url(text) => Some(text.clone()),
        serde_json::Value::Array(items) => items.iter().find_map(find_url_in_json),
        serde_json::Value::Object(map) => {
            for key in ["url", "music", "playUrl", "play_url", "src"] {
                if let Some(url) = map.get(key).and_then(find_url_in_json) {
                    return Some(url);
                }
            }
            map.values().find_map(find_url_in_json)
        }
        _ => None,
    }
}

fn is_http_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

fn append_query_params(url: &str, params: &[(&str, String)]) -> String {
    let query = params
        .iter()
        .map(|(key, value)| format!("{}={}", encode_component(key), encode_component(value)))
        .collect::<Vec<_>>()
        .join("&");
    if query.is_empty() {
        return url.to_owned();
    }
    let separator = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separator}{query}")
}

fn qishui_first_data(value: &serde_json::Value) -> Option<&serde_json::Value> {
    let data = value.get("data")?;
    match data {
        serde_json::Value::Array(items) => items.first(),
        serde_json::Value::Object(map) => map
            .get("0")
            .or_else(|| map.get("song"))
            .or_else(|| map.get("info")),
        _ => None,
    }
}

fn qishui_song_id(value: &serde_json::Value) -> Option<String> {
    for key in [
        "id",
        "songmid",
        "songId",
        "songid",
        "hash",
        "rid",
        "mid",
        "strMediaMid",
        "mediaId",
    ] {
        if let Some(id) = json_string(value.get(key)) {
            if !id.is_empty() {
                return Some(id);
            }
        }
    }
    None
}

fn normalize_qishui_quality(quality: SourceQuality) -> &'static str {
    match quality {
        SourceQuality::K128 => "low",
        SourceQuality::K320 => "standard",
        SourceQuality::Flac => "lossless",
        SourceQuality::Flac24Bit => "flac24bit",
    }
}

fn normalize_qishui_song_info(raw: &serde_json::Value) -> SourceSearchResult {
    let id = json_string(raw.get("id").or_else(|| raw.get("vid"))).unwrap_or_default();
    let title = json_string(raw.get("name")).unwrap_or_else(|| "未知歌曲".to_owned());
    let artist = json_string(raw.get("artists")).unwrap_or_else(|| "未知歌手".to_owned());
    let album = json_string(raw.get("album")).filter(|value| !value.is_empty());
    let duration_seconds = raw
        .get("duration")
        .and_then(json_u64)
        .map(|duration_ms| duration_ms / 1000);
    let cover_url =
        json_string(raw.get("cover").or_else(|| raw.get("pic"))).filter(|value| !value.is_empty());
    let raw_info = serde_json::json!({
        "id": id,
        "songmid": id,
        "hash": id,
        "name": title,
        "singer": artist,
        "albumName": album.clone().unwrap_or_default(),
        "duration": duration_seconds.unwrap_or_default(),
        "pic": cover_url.clone().unwrap_or_default(),
        "_raw": raw,
    });

    SourceSearchResult {
        id,
        source: QISHUI_SOURCE_ID.to_owned(),
        title,
        artist,
        album,
        duration_seconds,
        cover_url,
        raw_info,
    }
}

fn json_string(value: Option<&serde_json::Value>) -> Option<String> {
    match value? {
        serde_json::Value::String(value) => Some(value.clone()),
        serde_json::Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn json_u64(value: &serde_json::Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LxJsObfuscationReport {
    pub likely_obfuscated: bool,
    pub hex_identifier_count: usize,
    pub long_line_count: usize,
    pub signals: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LxJsDeobfuscationReport {
    pub string_table_len: usize,
    pub decoder_count: usize,
    pub rotation: Option<usize>,
    pub decoded_strings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StringDecoderSpec {
    name: String,
    offset: usize,
    kind: StringDecoderKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StringDecoderKind {
    Base64Percent,
    Rc4,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DecoderCall {
    decoder: StringDecoderSpec,
    index: usize,
    key: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum LxJsImportError {
    #[error("failed to read LX JS source {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("failed to parse LX JS source {path}: {diagnostics:?}")]
    Parse {
        path: String,
        diagnostics: Vec<String>,
    },
}

pub fn analyze_lx_js_file(path: impl AsRef<Path>) -> Result<LxJsImportReport, LxJsImportError> {
    let path = path.as_ref();
    let source = fs::read_to_string(path).map_err(|source| LxJsImportError::Read {
        path: path.display().to_string(),
        source,
    })?;

    analyze_lx_js_source(path, &source)
}

pub fn analyze_lx_js_source(
    path: impl AsRef<Path>,
    source: &str,
) -> Result<LxJsImportReport, LxJsImportError> {
    let path = path.as_ref();
    let parse = parse_with_oxc(path, source)?;
    let obfuscation = scan_obfuscation(source);
    let metadata = parse_metadata(source);
    let deobfuscation = deobfuscate_string_literals(source, &metadata);
    let contract = scan_lx_contract(source, &deobfuscation, obfuscation.likely_obfuscated);
    let endpoint = scan_endpoint_report(source, &deobfuscation.decoded_strings);
    let manifest = build_import_manifest(&metadata, &contract, &obfuscation, &deobfuscation);

    Ok(LxJsImportReport {
        file_name: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("<memory>")
            .to_owned(),
        metadata,
        parse,
        contract,
        endpoint,
        obfuscation,
        deobfuscation,
        manifest,
    })
}

fn parse_with_oxc(path: &Path, source: &str) -> Result<JsParseReport, LxJsImportError> {
    let allocator = Allocator::default();
    let source_type = SourceType::from_path(path).unwrap_or_else(|_| SourceType::mjs());
    let parser_return = Parser::new(&allocator, source, source_type).parse();
    let diagnostics = parser_return
        .diagnostics
        .iter()
        .map(|diagnostic| format!("{diagnostic:?}"))
        .collect::<Vec<_>>();

    if parser_return.panicked || !diagnostics.is_empty() {
        return Err(LxJsImportError::Parse {
            path: path.display().to_string(),
            diagnostics,
        });
    }

    Ok(JsParseReport {
        parsed_with_oxc: true,
        top_level_statement_count: parser_return.program.body.len(),
        comment_count: parser_return.program.comments.len(),
        diagnostics,
    })
}

fn parse_metadata(source: &str) -> LxJsMetadata {
    let mut metadata = LxJsMetadata::default();

    for line in source.lines().take(120) {
        let line = line.trim().trim_start_matches('*').trim();
        let Some(tag) = line.strip_prefix('@') else {
            continue;
        };
        let Some((key, value)) = split_tag(tag) else {
            continue;
        };

        match key {
            "name" => metadata.name = Some(value.to_owned()),
            "description" => metadata.description = Some(value.to_owned()),
            "version" => metadata.version = Some(value.to_owned()),
            "author" => metadata.author = Some(value.to_owned()),
            "homepage" => metadata.homepage = Some(value.to_owned()),
            "update_url" => metadata.update_url = Some(value.to_owned()),
            _ => {}
        }
        metadata.raw_tags.insert(key.to_owned(), value.to_owned());
    }

    metadata
}

fn split_tag(tag: &str) -> Option<(&str, &str)> {
    let mut parts = tag.splitn(2, char::is_whitespace);
    let key = parts.next()?.trim();
    let value = parts.next()?.trim();
    if key.is_empty() || value.is_empty() {
        return None;
    }
    Some((key, value))
}

fn scan_lx_contract(
    source: &str,
    deobfuscation: &LxJsDeobfuscationReport,
    likely_obfuscated: bool,
) -> LxJsContractReport {
    let declared_actions = scan_declared_actions(source, &deobfuscation.decoded_strings);
    let declared_qualities = scan_declared_qualities(source, &deobfuscation.decoded_strings);
    let declared_sources = scan_declared_sources(source, &declared_actions, &declared_qualities);
    let references_qualitys = source.contains("qualitys") || source.contains("qualities");

    LxJsContractReport {
        uses_global_lx: source.contains("globalThis['lx']") || source.contains("globalThis.lx"),
        uses_event_names: source.contains("EVENT_NAMES"),
        uses_lx_request: source.contains("request,")
            || source.contains("request}")
            || source.contains("request(")
            || source.contains("request,"),
        registers_request_handler: source.contains("EVENT_NAMES.request")
            || source.contains("EVENT_NAMES[")
            || source.contains("on(EVENT_NAMES"),
        sends_any_lx_event: source.contains("send(EVENT_NAMES"),
        sends_inited_event_literal: source.contains("EVENT_NAMES.inited")
            || contains_string_literal(source, "inited")
            || deobfuscation
                .decoded_strings
                .iter()
                .any(|value| value == "inited"),
        sends_update_alert_event_literal: source.contains("EVENT_NAMES.updateAlert")
            || contains_string_literal(source, "updateAlert")
            || deobfuscation
                .decoded_strings
                .iter()
                .any(|value| value == "updateAlert"),
        declared_actions,
        declared_qualities: declared_qualities.clone(),
        declared_sources,
        requires_deobfuscation_for_full_manifest: likely_obfuscated
            || (references_qualitys && declared_qualities.is_empty()),
    }
}

fn build_import_manifest(
    metadata: &LxJsMetadata,
    contract: &LxJsContractReport,
    obfuscation: &LxJsObfuscationReport,
    deobfuscation: &LxJsDeobfuscationReport,
) -> ImportedLxManifest {
    let display_name = metadata
        .name
        .as_deref()
        .unwrap_or("Imported LX Source")
        .to_owned();
    let mut warnings = Vec::new();

    if contract.declared_sources.is_empty() {
        warnings.push("no LX source catalog entries could be inferred".to_owned());
    }
    if contract.requires_deobfuscation_for_full_manifest {
        warnings.push("source requires Rust porting before it can resolve playback".to_owned());
    }
    if obfuscation.likely_obfuscated && deobfuscation.rotation.is_none() {
        warnings.push("obfuscated string table rotation could not be inferred".to_owned());
    }
    if !contract.sends_inited_event_literal {
        warnings.push("inited event is not visible as a plain LX event literal".to_owned());
    }

    ImportedLxManifest {
        provider_id: provider_id_from_metadata(metadata),
        display_name,
        version: metadata.version.clone(),
        author: metadata.author.clone(),
        homepage: metadata.homepage.clone(),
        update_url: metadata.update_url.clone(),
        sources: contract.declared_sources.clone(),
        requires_rust_port: true,
        warnings,
    }
}

fn provider_id_from_metadata(metadata: &LxJsMetadata) -> String {
    let seed = metadata
        .name
        .as_deref()
        .or(metadata.author.as_deref())
        .unwrap_or("imported-lx-source");
    let mut normalized = String::new();
    let mut previous_dash = false;

    for character in seed.chars() {
        if character.is_ascii_alphanumeric() {
            normalized.push(character.to_ascii_lowercase());
            previous_dash = false;
        } else if !previous_dash && !normalized.is_empty() {
            normalized.push('-');
            previous_dash = true;
        }
    }

    let normalized = normalized.trim_matches('-');
    let fingerprint = metadata_fingerprint(metadata);
    if normalized.is_empty() {
        format!("imported-lx-{fingerprint}")
    } else {
        format!("imported-lx-{normalized}-{fingerprint}")
    }
}

fn metadata_fingerprint(metadata: &LxJsMetadata) -> String {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET;
    for value in [
        metadata.name.as_deref(),
        metadata.version.as_deref(),
        metadata.author.as_deref(),
        metadata.homepage.as_deref(),
        metadata.update_url.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        for byte in value.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        hash ^= u64::from(0xff_u8);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    format!("{hash:016x}")
}

fn extract_track_id(info: &serde_json::Value) -> Option<String> {
    for pointer in [
        "/id",
        "/songmid",
        "/songid",
        "/songId",
        "/hash",
        "/rid",
        "/mid",
        "/strMediaMid",
        "/mediaId",
        "/musicInfo/id",
        "/musicInfo/songmid",
        "/musicInfo/songid",
        "/musicInfo/songId",
        "/musicInfo/hash",
        "/musicInfo/rid",
        "/musicInfo/mid",
        "/musicInfo/strMediaMid",
        "/musicInfo/mediaId",
    ] {
        let Some(value) = info.pointer(pointer) else {
            continue;
        };
        if let Some(text) = value.as_str().filter(|value| !value.is_empty()) {
            return Some(text.to_owned());
        }
        if let Some(number) = value.as_i64() {
            return Some(number.to_string());
        }
        if let Some(number) = value.as_u64() {
            return Some(number.to_string());
        }
    }
    None
}

fn quality_to_template_level(quality: SourceQuality) -> &'static str {
    match quality {
        SourceQuality::Flac | SourceQuality::Flac24Bit => "lossless",
        SourceQuality::K320 => "exhigh",
        SourceQuality::K128 => "standard",
    }
}

fn encode_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(hex_digit(byte >> 4));
            encoded.push(hex_digit(byte & 0x0f));
        }
    }
    encoded
}

fn hex_digit(nibble: u8) -> char {
    match nibble {
        0..=9 => char::from(b'0' + nibble),
        10..=15 => char::from(b'A' + (nibble - 10)),
        _ => '0',
    }
}

fn scan_declared_actions(source: &str, decoded_strings: &[String]) -> Vec<SourceAction> {
    let mut actions = Vec::new();
    if contains_string_literal(source, "musicSearch")
        || source.contains("action === \"musicSearch\"")
        || source.contains("action === \"search\"")
        || decoded_strings
            .iter()
            .any(|value| value == "musicSearch" || value == "search")
    {
        push_unique_action(&mut actions, SourceAction::MusicSearch);
    }
    if contains_string_literal(source, "musicUrl")
        || source.contains("action === \"musicUrl\"")
        || decoded_strings.iter().any(|value| value == "musicUrl")
    {
        push_unique_action(&mut actions, SourceAction::MusicUrl);
    }
    if contains_string_literal(source, "lyric")
        || source.contains("action === \"lyric\"")
        || decoded_strings.iter().any(|value| value == "lyric")
    {
        push_unique_action(&mut actions, SourceAction::Lyric);
    }
    if contains_string_literal(source, "pic")
        || source.contains("action === \"pic\"")
        || decoded_strings.iter().any(|value| value == "pic")
    {
        push_unique_action(&mut actions, SourceAction::Pic);
    }
    actions
}

fn scan_declared_qualities(source: &str, decoded_strings: &[String]) -> Vec<SourceQuality> {
    let mut qualities = Vec::new();
    for (needle, aliases, quality) in [
        ("128k", &["standard"][..], SourceQuality::K128),
        ("320k", &["exhigh"][..], SourceQuality::K320),
        ("flac", &["lossless"][..], SourceQuality::Flac),
        (
            "flac24bit",
            &["hires", "master", "atmos"][..],
            SourceQuality::Flac24Bit,
        ),
        (
            "24bit",
            &["hires", "master", "atmos"][..],
            SourceQuality::Flac24Bit,
        ),
    ] {
        if contains_string_literal(source, needle)
            || decoded_strings.iter().any(|value| value == needle)
            || aliases
                .iter()
                .any(|alias| decoded_strings.iter().any(|value| value == alias))
        {
            push_unique_quality(&mut qualities, quality);
        }
    }
    qualities
}

fn scan_declared_sources(
    source: &str,
    declared_actions: &[SourceAction],
    declared_qualities: &[SourceQuality],
) -> BTreeMap<String, ImportedLxSourceInfo> {
    let mut sources = BTreeMap::new();
    for source_id in LX_SOURCE_KEYS {
        if source_mentions_lx_key(source, source_id) {
            let qualities = if *source_id == LX_SOURCE_LOCAL {
                Vec::new()
            } else {
                declared_qualities.to_vec()
            };
            sources.insert(
                (*source_id).to_owned(),
                ImportedLxSourceInfo {
                    id: (*source_id).to_owned(),
                    name: infer_source_display_name(source, source_id),
                    kind: LX_SOURCE_KIND_MUSIC.to_owned(),
                    actions: declared_actions.to_vec(),
                    qualities,
                },
            );
        }
    }
    sources
}

fn infer_source_display_name(source: &str, source_id: &str) -> String {
    for (id, display_name) in [
        (LX_SOURCE_WY, "网易云音乐"),
        (LX_SOURCE_TX, "QQ音乐"),
        (LX_SOURCE_KW, "酷我音乐"),
        (LX_SOURCE_KG, "酷狗音乐"),
        (LX_SOURCE_MG, "咪咕音乐"),
        (LX_SOURCE_LOCAL, "本地音乐"),
    ] {
        if source_id == id && source.contains(display_name) {
            return display_name.to_owned();
        }
    }

    match source_id {
        LX_SOURCE_WY => "NetEase".to_owned(),
        LX_SOURCE_TX => "QQ Music".to_owned(),
        LX_SOURCE_KW => "Kuwo".to_owned(),
        LX_SOURCE_KG => "Kugou".to_owned(),
        LX_SOURCE_MG => "Migu".to_owned(),
        LX_SOURCE_LOCAL => "Local Music".to_owned(),
        _ => source_id.to_owned(),
    }
}

fn scan_endpoint_report(source: &str, decoded_strings: &[String]) -> LxJsEndpointReport {
    let mut urls = Vec::new();
    collect_urls_from_text(source, &mut urls);
    for decoded in decoded_strings {
        collect_urls_from_text(decoded, &mut urls);
    }

    let templates = scan_url_templates(source);
    let template_count = urls
        .iter()
        .filter(|url| is_url_template(url))
        .count()
        .max(templates.len());
    let decoded_url_count = decoded_strings
        .iter()
        .filter(|value| value.starts_with("http://") || value.starts_with("https://"))
        .count();
    let domains = urls.iter().filter_map(|url| extract_domain(url)).fold(
        Vec::new(),
        |mut domains, domain| {
            if !domains.contains(&domain) {
                domains.push(domain);
            }
            domains
        },
    );

    LxJsEndpointReport {
        urls,
        domains,
        templates,
        template_count,
        decoded_url_count,
    }
}

fn scan_url_templates(source: &str) -> Vec<LxUrlTemplate> {
    let mut templates = Vec::new();
    let mut cursor = 0;
    while let Some(offset) = source[cursor..].find("URL_TEMPLATES") {
        let name_end = cursor + offset + "URL_TEMPLATES".len();
        let Some(const_start) = source[..name_end].rfind("const ") else {
            cursor = name_end;
            continue;
        };
        let family = source[const_start + "const ".len()..name_end]
            .trim()
            .trim_end_matches("URL_TEMPLATES")
            .trim_end_matches('_')
            .to_ascii_lowercase();
        let Some(object_start_offset) = source[name_end..].find('{') else {
            cursor = name_end;
            continue;
        };
        let object_start = name_end + object_start_offset;
        let Some(object_end) = find_matching_brace(source, object_start) else {
            cursor = name_end;
            continue;
        };
        let body = &source[object_start + 1..object_end];
        collect_platform_templates(&family, body, &mut templates);
        cursor = object_end.saturating_add(1);
    }
    templates
}

fn collect_platform_templates(family: &str, body: &str, templates: &mut Vec<LxUrlTemplate>) {
    for line in body.lines() {
        let line = line.trim().trim_end_matches(',');
        let Some((raw_key, raw_value)) = line.split_once(':') else {
            continue;
        };
        let source_id = raw_key.trim().trim_matches(['\'', '"']).to_owned();
        if !LX_SOURCE_KEYS.contains(&source_id.as_str()) {
            continue;
        }
        let raw_value = raw_value.trim();
        if raw_value.is_empty() || !raw_value.contains("http") {
            continue;
        }
        let url = raw_value
            .trim_matches(['\'', '"', '`'])
            .trim_end_matches(',')
            .to_owned();
        if !is_url_template(&url) {
            continue;
        }
        templates.push(LxUrlTemplate {
            family: family.to_owned(),
            source_id,
            domain: extract_domain(&url),
            has_track_id_placeholder: url.contains("{id}") || url.contains("${id}"),
            has_quality_placeholder: url.contains("{level}") || url.contains("${level}"),
            url,
        });
    }
}

fn is_url_template(url: &str) -> bool {
    url.contains("{id}") || url.contains("{level}") || url.contains("${")
}

fn find_matching_brace(source: &str, open: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    if *bytes.get(open)? != b'{' {
        return None;
    }
    let mut depth = 0usize;
    let mut index = open;
    let mut quote = None;
    let mut escaped = false;

    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active_quote {
                quote = None;
            }
            index += 1;
            continue;
        }

        match byte {
            b'\'' | b'"' | b'`' => quote = Some(byte),
            b'{' => depth += 1,
            b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

fn collect_urls_from_text(text: &str, urls: &mut Vec<String>) {
    let bytes = text.as_bytes();
    let mut cursor = 0;
    while cursor < text.len() {
        let http_offset = earliest_offset(
            text[cursor..].find("http://"),
            text[cursor..].find("https://"),
        );
        let Some(offset) = http_offset else {
            break;
        };
        let start = cursor + offset;
        let mut end = start;
        while end < bytes.len()
            && !bytes[end].is_ascii_whitespace()
            && !matches!(bytes[end], b'\'' | b'"' | b'`' | b'<' | b'>' | b')' | b']')
        {
            end += 1;
        }
        let url = text[start..end]
            .trim_end_matches([',', ';', '.'])
            .to_owned();
        if !url.is_empty() && !urls.contains(&url) {
            urls.push(url);
        }
        cursor = end.saturating_add(1);
    }
}

fn earliest_offset(left: Option<usize>, right: Option<usize>) -> Option<usize> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn extract_domain(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let domain = rest
        .split(['/', '?', '#'])
        .next()?
        .trim()
        .trim_end_matches(':');
    if domain.is_empty() {
        None
    } else {
        Some(domain.to_owned())
    }
}

fn scan_obfuscation(source: &str) -> LxJsObfuscationReport {
    let hex_identifier_count = source.matches("_0x").count();
    let long_line_count = source.lines().filter(|line| line.len() > 1_000).count();
    let mut signals = Vec::new();

    if hex_identifier_count >= 20 {
        signals.push(format!(
            "high _0x-style identifier density ({hex_identifier_count})"
        ));
    }
    if long_line_count > 0 {
        signals.push(format!("contains {long_line_count} minified long line(s)"));
    }
    if source.contains("while(!![])") || source.contains("while (!![])") {
        signals.push("string-array rotation loop".to_owned());
    }
    if source.contains("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789+/=")
        && source.contains("decodeURIComponent")
    {
        signals.push("base64 percent-decoder string table".to_owned());
    }
    if source.contains("0x100") && source.contains("charCodeAt") && source.contains("fromCharCode")
    {
        signals.push("RC4-like string decoder".to_owned());
    }
    if source.contains("qualitys") && scan_declared_qualities(source, &[]).is_empty() {
        signals.push("quality labels are not present as plain string literals".to_owned());
    }

    LxJsObfuscationReport {
        likely_obfuscated: !signals.is_empty(),
        hex_identifier_count,
        long_line_count,
        signals,
    }
}

fn deobfuscate_string_literals(source: &str, metadata: &LxJsMetadata) -> LxJsDeobfuscationReport {
    let string_table = extract_largest_string_array(source);
    let decoder_specs = extract_decoder_specs(source);
    if string_table.is_empty() || decoder_specs.is_empty() {
        return LxJsDeobfuscationReport {
            string_table_len: string_table.len(),
            decoder_count: decoder_specs.len(),
            ..LxJsDeobfuscationReport::default()
        };
    }

    let aliases = extract_decoder_aliases(source, &decoder_specs);
    let calls = extract_decoder_calls(source, &aliases);
    if calls.is_empty() {
        return LxJsDeobfuscationReport {
            string_table_len: string_table.len(),
            decoder_count: decoder_specs.len(),
            ..LxJsDeobfuscationReport::default()
        };
    }

    let rotation = infer_rotation(&string_table, &calls, metadata);
    let decoded_strings = decode_calls(&string_table, &calls, rotation);

    LxJsDeobfuscationReport {
        string_table_len: string_table.len(),
        decoder_count: decoder_specs.len(),
        rotation,
        decoded_strings,
    }
}

fn extract_largest_string_array(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut best = Vec::new();
    let mut cursor = 0;

    while cursor < bytes.len() {
        let Some(open_offset) = source[cursor..].find('[') else {
            break;
        };
        let mut index = cursor + open_offset + 1;
        let mut values = Vec::new();

        loop {
            index = skip_ascii_whitespace(bytes, index);
            if index >= bytes.len() {
                break;
            }
            if bytes[index] == b']' {
                if values.len() > best.len() {
                    best = values;
                }
                break;
            }
            if bytes[index] != b'\'' && bytes[index] != b'"' {
                break;
            }

            let Some((value, next_index)) = parse_js_string_literal(source, index) else {
                break;
            };
            values.push(value);
            index = skip_ascii_whitespace(bytes, next_index);

            if index < bytes.len() && bytes[index] == b',' {
                index += 1;
                continue;
            }
            if index < bytes.len() && bytes[index] == b']' && values.len() > best.len() {
                best = values;
            }
            break;
        }

        cursor += open_offset + 1;
    }

    best
}

fn parse_js_string_literal(source: &str, start: usize) -> Option<(String, usize)> {
    let bytes = source.as_bytes();
    let quote = *bytes.get(start)?;
    if quote != b'\'' && quote != b'"' {
        return None;
    }

    let mut value = String::new();
    let mut index = start + 1;
    while index < bytes.len() {
        let byte = bytes[index];
        index += 1;
        if byte == quote {
            return Some((value, index));
        }
        if byte != b'\\' {
            value.push(char::from(byte));
            continue;
        }

        let escape = *bytes.get(index)?;
        index += 1;
        match escape {
            b'n' => value.push('\n'),
            b'r' => value.push('\r'),
            b't' => value.push('\t'),
            b'0' => value.push('\0'),
            b'x' => {
                let hex = source.get(index..index + 2)?;
                let code = u8::from_str_radix(hex, 16).ok()?;
                value.push(char::from(code));
                index += 2;
            }
            b'u' => {
                let hex = source.get(index..index + 4)?;
                let code = u32::from_str_radix(hex, 16).ok()?;
                value.push(char::from_u32(code)?);
                index += 4;
            }
            _ => value.push(char::from(escape)),
        }
    }

    None
}

fn skip_ascii_whitespace(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    index
}

fn extract_decoder_specs(source: &str) -> Vec<StringDecoderSpec> {
    let mut specs = Vec::new();
    let mut cursor = 0;
    while let Some(offset) = source[cursor..].find("function _0x") {
        let function_start = cursor + offset;
        let Some(name_start) = source[function_start..].find("_0x") else {
            break;
        };
        let name_start = function_start + name_start;
        let name_end = read_identifier_end(source, name_start);
        let name = source[name_start..name_end].to_owned();
        let body = &source[name_end..source.len().min(name_end + 8_000)];
        let Some(offset_index) = body.find("-0x").or_else(|| body.find("- 0x")) else {
            cursor = name_end;
            continue;
        };
        let offset_slice = &body[offset_index..];
        let Some(hex_start) = offset_slice.find("0x") else {
            cursor = name_end;
            continue;
        };
        let hex_start = offset_index + hex_start + 2;
        let hex = body[hex_start..]
            .chars()
            .take_while(char::is_ascii_hexdigit)
            .collect::<String>();
        let Ok(offset) = usize::from_str_radix(&hex, 16) else {
            cursor = name_end;
            continue;
        };
        let kind = if body.contains("0x100")
            && body.contains("charCodeAt")
            && body.contains("fromCharCode")
        {
            StringDecoderKind::Rc4
        } else {
            StringDecoderKind::Base64Percent
        };
        if !specs
            .iter()
            .any(|spec: &StringDecoderSpec| spec.name == name)
        {
            specs.push(StringDecoderSpec { name, offset, kind });
        }
        cursor = name_end;
    }
    specs
}

fn extract_decoder_aliases(
    source: &str,
    decoder_specs: &[StringDecoderSpec],
) -> BTreeMap<String, StringDecoderSpec> {
    let mut aliases = BTreeMap::new();
    for spec in decoder_specs {
        aliases.insert(spec.name.clone(), spec.clone());
    }

    let prefix = &source[..source.len().min(2_000)];
    let mut cursor = 0;
    while let Some(offset) = prefix[cursor..].find("_0x") {
        let lhs_start = cursor + offset;
        let lhs_end = read_identifier_end(prefix, lhs_start);
        let lhs = &prefix[lhs_start..lhs_end];
        let rest = &prefix[lhs_end..];
        let Some(equal_offset) = rest.find('=') else {
            cursor = lhs_end;
            continue;
        };
        let rhs_start = skip_ascii_whitespace(prefix.as_bytes(), lhs_end + equal_offset + 1);
        if !prefix[rhs_start..].starts_with("_0x") {
            cursor = lhs_end;
            continue;
        }
        let rhs_end = read_identifier_end(prefix, rhs_start);
        let rhs = &prefix[rhs_start..rhs_end];
        if let Some(spec) = aliases.get(rhs).cloned() {
            aliases.insert(lhs.to_owned(), spec);
        }
        cursor = rhs_end;
    }

    aliases
}

fn extract_decoder_calls(
    source: &str,
    aliases: &BTreeMap<String, StringDecoderSpec>,
) -> Vec<DecoderCall> {
    let mut calls = Vec::new();
    for (alias, spec) in aliases {
        let pattern = format!("{alias}(");
        let mut cursor = 0;
        while let Some(offset) = source[cursor..].find(&pattern) {
            let call_start = cursor + offset + pattern.len();
            if let Some((index, key)) = parse_decoder_call_args(source, call_start, spec.kind) {
                calls.push(DecoderCall {
                    decoder: spec.clone(),
                    index,
                    key,
                });
            }
            cursor = call_start;
        }
    }
    calls
}

fn parse_decoder_call_args(
    source: &str,
    args_start: usize,
    kind: StringDecoderKind,
) -> Option<(usize, Option<String>)> {
    let bytes = source.as_bytes();
    let index_start = skip_ascii_whitespace(bytes, args_start);
    let (index, index_end) = parse_js_number(source, index_start)?;
    if kind == StringDecoderKind::Base64Percent {
        let close = skip_ascii_whitespace(bytes, index_end);
        if close < bytes.len() && bytes[close] == b')' {
            return Some((index, None));
        }
    }

    let comma = skip_ascii_whitespace(bytes, index_end);
    if comma >= bytes.len() || bytes[comma] != b',' {
        return None;
    }
    let key_start = skip_ascii_whitespace(bytes, comma + 1);
    let (key, _) = parse_js_string_literal(source, key_start)?;
    Some((index, Some(key)))
}

fn parse_js_number(source: &str, start: usize) -> Option<(usize, usize)> {
    let tail = source.get(start..)?;
    if tail.starts_with("0x") || tail.starts_with("0X") {
        let digits = tail[2..]
            .chars()
            .take_while(char::is_ascii_hexdigit)
            .collect::<String>();
        if digits.is_empty() {
            return None;
        }
        let value = usize::from_str_radix(&digits, 16).ok()?;
        return Some((value, start + 2 + digits.len()));
    }

    let digits = tail
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    if digits.is_empty() {
        return None;
    }
    let value = digits.parse::<usize>().ok()?;
    Some((value, start + digits.len()))
}

fn read_identifier_end(source: &str, start: usize) -> usize {
    source[start..]
        .find(|character: char| {
            !(character == '_' || character == 'x' || character.is_ascii_hexdigit())
        })
        .map(|offset| start + offset)
        .unwrap_or(source.len())
}

fn infer_rotation(
    string_table: &[String],
    calls: &[DecoderCall],
    metadata: &LxJsMetadata,
) -> Option<usize> {
    let mut anchors = Vec::new();
    if let Some(version) = metadata.version.as_deref() {
        anchors.push(version.strip_prefix('v').unwrap_or(version).to_owned());
    }
    anchors.extend([
        "musicUrl".to_owned(),
        "request".to_owned(),
        "inited".to_owned(),
        "128k".to_owned(),
        "320k".to_owned(),
        "flac".to_owned(),
    ]);

    let mut best_rotation = None;
    let mut best_score = 0;
    for rotation in 0..string_table.len() {
        let score = calls
            .iter()
            .filter_map(|call| decode_call(string_table, call, Some(rotation)))
            .filter(|value| anchors.iter().any(|anchor| value == anchor))
            .count();
        if score > best_score {
            best_score = score;
            best_rotation = Some(rotation);
        }
    }

    best_rotation.filter(|_| best_score > 0)
}

fn decode_calls(
    string_table: &[String],
    calls: &[DecoderCall],
    rotation: Option<usize>,
) -> Vec<String> {
    let mut decoded = Vec::new();
    for call in calls {
        if let Some(value) = decode_call(string_table, call, rotation) {
            if is_useful_decoded_string(&value) && !decoded.contains(&value) {
                decoded.push(value);
            }
        }
    }
    decoded
}

fn decode_call(
    string_table: &[String],
    call: &DecoderCall,
    rotation: Option<usize>,
) -> Option<String> {
    if string_table.is_empty() || call.index < call.decoder.offset {
        return None;
    }
    let index = call.index - call.decoder.offset;
    if index >= string_table.len() {
        return None;
    }

    let rotated_index = (index + rotation.unwrap_or(0)) % string_table.len();
    let encoded = string_table.get(rotated_index)?;
    match call.decoder.kind {
        StringDecoderKind::Base64Percent => decode_base64_percent(encoded),
        StringDecoderKind::Rc4 => decode_rc4(encoded, call.key.as_deref()?),
    }
}

fn decode_base64_percent(encoded: &str) -> Option<String> {
    let bytes = decode_obfuscator_base64(encoded);
    percent_decode_bytes(&bytes)
}

fn decode_obfuscator_base64(encoded: &str) -> Vec<u8> {
    const ALPHABET: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789+/=";
    let mut output = Vec::new();
    let mut char_count = 0usize;
    let mut buffer = 0usize;

    for character in encoded.chars() {
        let Some(index) = ALPHABET.find(character) else {
            continue;
        };
        if index == 64 {
            break;
        }

        buffer = if char_count.is_multiple_of(4) {
            index
        } else {
            buffer * 64 + index
        };
        char_count += 1;

        if char_count % 4 != 1 {
            let shift = ((-2 * (char_count as isize)) & 6) as usize;
            output.push(((buffer >> shift) & 0xff) as u8);
        }
    }

    output
}

fn percent_decode_bytes(bytes: &[u8]) -> Option<String> {
    let mut decoded = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok()?;
            let value = u8::from_str_radix(hex, 16).ok()?;
            decoded.push(value);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn decode_rc4(encoded: &str, key: &str) -> Option<String> {
    let decoded = decode_base64_percent(encoded)?;
    if key.is_empty() {
        return None;
    }

    let key = key.as_bytes();
    let mut state = (0..=255).collect::<Vec<u8>>();
    let mut j = 0usize;
    for i in 0..256 {
        j = (j + usize::from(state[i]) + usize::from(key[i % key.len()])) % 256;
        state.swap(i, j);
    }

    let mut i = 0usize;
    j = 0;
    let mut output = String::with_capacity(decoded.len());
    for character in decoded.chars() {
        i = (i + 1) % 256;
        j = (j + usize::from(state[i])) % 256;
        state.swap(i, j);
        let key_index = (usize::from(state[i]) + usize::from(state[j])) % 256;
        let codepoint = u32::from(character) ^ u32::from(state[key_index]);
        output.push(char::from_u32(codepoint)?);
    }

    Some(output)
}

fn is_useful_decoded_string(value: &str) -> bool {
    if value.is_empty() || value.len() > 240 {
        return false;
    }
    value.chars().all(|character| {
        !character.is_control() || character == '\n' || character == '\r' || character == '\t'
    })
}

fn source_mentions_lx_key(source: &str, key: &str) -> bool {
    contains_string_literal(source, key) || source.contains(&format!("{key}:"))
}

fn contains_string_literal(source: &str, value: &str) -> bool {
    source.contains(&format!("\"{value}\"")) || source.contains(&format!("'{value}'"))
}

fn push_unique_action(actions: &mut Vec<SourceAction>, action: SourceAction) {
    if !actions.contains(&action) {
        actions.push(action);
    }
}

fn push_unique_quality(qualities: &mut Vec<SourceQuality>, quality: SourceQuality) {
    if !qualities.contains(&quality) {
        qualities.push(quality);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_runtime::{
        mock_music_url_request, DefaultSourceHost, SourceCapability, SourceKind, SourceRuntime,
    };
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    const LX_JS_REFERENCE_SOURCES: &[(&str, &str)] = &[
        ("quantouya-aggregate-v4.1.js", "全豆要"),
        ("nianxin-v1.0.1.js", "念心音源"),
        ("changqing-svip-v1.2.0.js", "长青SVIP音源"),
    ];

    fn fixture_path(file_name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/lx-js-sources")
            .join(file_name)
    }

    fn spawn_http_response(content_type: &str, body: &str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("test HTTP server should bind");
        let address = listener
            .local_addr()
            .expect("test HTTP server should have address");
        let content_type = content_type.to_owned();
        let body = body.to_owned();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("test HTTP server should accept");
            let mut buffer = [0_u8; 1024];
            let _ = stream.read(&mut buffer);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .expect("test HTTP response should write");
        });
        format!("http://{address}/resolve?id={{id}}&level={{level}}")
    }

    fn test_http_client() -> reqwest::blocking::Client {
        reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .expect("test HTTP client should build")
    }

    fn test_runtime() -> SourceRuntime {
        let host = Arc::new(DefaultSourceHost::with_client(
            test_http_client(),
            Duration::from_secs(2),
            4 * 1024 * 1024,
        ));
        SourceRuntime::with_host(host, [SourceCapability::NetworkAny])
    }

    fn spawn_qishui_api(expected_requests: usize) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("qsvip test server should bind");
        let address = listener
            .local_addr()
            .expect("qsvip test server should have address");
        thread::spawn(move || {
            for _ in 0..expected_requests {
                let (mut stream, _) = listener.accept().expect("qsvip server should accept");
                let mut buffer = [0_u8; 4096];
                let size = stream.read(&mut buffer).unwrap_or_default();
                let request = String::from_utf8_lossy(&buffer[..size]);
                let body = if request.contains("act=search") {
                    serde_json::json!({
                        "data": {
                            "total": 1,
                            "lists": [
                                {
                                    "id": "song-1",
                                    "name": "晴天",
                                    "artists": "周杰伦",
                                    "album": "叶惠美",
                                    "duration": 269000,
                                    "cover": "https://img.example.test/cover.jpg"
                                }
                            ]
                        }
                    })
                    .to_string()
                } else {
                    serde_json::json!({
                        "data": [
                            {
                                "url": "https://cdn.example.test/song-1.mp3",
                                "lyric": "[00:00.00]晴天"
                            }
                        ]
                    })
                    .to_string()
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("qsvip response should write");
            }
        });
        format!("http://{address}/api")
    }

    #[test]
    fn fixture_lx_js_sources_should_parse_with_oxc_and_report_lx_contract() {
        for (file_name, display_name) in LX_JS_REFERENCE_SOURCES {
            let report = analyze_lx_js_file(fixture_path(file_name))
                .expect("fixture should parse and analyze");

            assert!(report.parse.parsed_with_oxc, "{file_name}");
            assert!(report.parse.top_level_statement_count > 0, "{file_name}");
            assert!(
                report
                    .metadata
                    .name
                    .as_deref()
                    .is_some_and(|name| name.contains(display_name)),
                "{file_name}"
            );
            assert!(report.contract.uses_global_lx, "{file_name}");
            assert!(report.contract.uses_event_names, "{file_name}");
            assert!(report.contract.registers_request_handler, "{file_name}");
            assert!(
                report
                    .contract
                    .declared_actions
                    .contains(&SourceAction::MusicUrl),
                "{file_name}"
            );
            assert!(report.contract.declared_sources.contains_key(LX_SOURCE_WY));
        }
    }

    #[test]
    fn readable_quantouya_source_should_expose_plain_actions_and_qualities() {
        let report = analyze_lx_js_file(fixture_path("quantouya-aggregate-v4.1.js"))
            .expect("fixture should analyze");

        assert!(!report.obfuscation.likely_obfuscated);
        assert!(report.contract.sends_inited_event_literal);
        assert!(report
            .contract
            .declared_actions
            .contains(&SourceAction::MusicUrl));
        assert!(report
            .contract
            .declared_actions
            .contains(&SourceAction::Lyric));
        assert!(report
            .contract
            .declared_qualities
            .contains(&SourceQuality::K128));
        assert!(report
            .contract
            .declared_qualities
            .contains(&SourceQuality::K320));
        assert!(report
            .contract
            .declared_qualities
            .contains(&SourceQuality::Flac));
        assert!(report
            .contract
            .declared_qualities
            .contains(&SourceQuality::Flac24Bit));
    }

    #[test]
    fn import_report_should_build_stable_manifest_and_source_catalog() {
        let report = analyze_lx_js_file(fixture_path("quantouya-aggregate-v4.1.js"))
            .expect("fixture should analyze");
        let catalog = report.manifest.to_source_catalog();

        assert!(report.manifest.provider_id.starts_with("imported-lx-"));
        assert_eq!(report.manifest.version.as_deref(), Some("v4.1"));
        assert!(report.manifest.requires_rust_port);
        assert_eq!(catalog.len(), 5);
        assert_eq!(catalog[LX_SOURCE_WY].name, "网易云音乐");
        assert_eq!(catalog[LX_SOURCE_WY].kind, SourceKind::Music);
        assert!(catalog[LX_SOURCE_WY]
            .actions
            .contains(&SourceAction::MusicUrl));
        assert!(catalog[LX_SOURCE_WY]
            .qualities
            .contains(&SourceQuality::Flac24Bit));
    }

    #[test]
    fn import_report_should_extract_endpoint_inventory_for_porting() {
        let report = analyze_lx_js_file(fixture_path("quantouya-aggregate-v4.1.js"))
            .expect("fixture should analyze");

        assert!(report.endpoint.urls.len() >= 10);
        assert!(report.endpoint.template_count >= 5);
        assert_eq!(report.endpoint.templates.len(), 10);
        assert!(report.endpoint.templates.iter().any(|template| {
            template.family == "nianxin"
                && template.source_id == LX_SOURCE_WY
                && template.has_track_id_placeholder
                && template.has_quality_placeholder
        }));
        assert!(report.endpoint.templates.iter().any(|template| {
            template.family == "changqing" && template.source_id == LX_SOURCE_MG
        }));
        assert!(report
            .endpoint
            .domains
            .iter()
            .any(|domain| domain == "music.nxinxz.com"));
        assert!(report
            .endpoint
            .domains
            .iter()
            .any(|domain| domain == "music-api.gdstudio.xyz"));
    }

    #[test]
    fn imported_manifest_provider_should_initialize_catalog_but_reject_requests_until_ported() {
        let report = analyze_lx_js_file(fixture_path("quantouya-aggregate-v4.1.js"))
            .expect("fixture should analyze");
        let provider = ImportedLxManifestProvider::new(report.manifest);
        let runtime = SourceRuntime::new();
        let init_report = runtime
            .initialize_provider(&provider)
            .expect("manifest provider should initialize catalog");

        assert!(init_report.source_id.starts_with("imported-lx-"));
        assert!(init_report.sources.contains_key(LX_SOURCE_WY));
        assert!(init_report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("Rust provider port")));

        let error = runtime
            .dispatch_request(&provider, mock_music_url_request(LX_SOURCE_WY, "track-1"))
            .expect_err("catalog-only manifest should reject playback requests");
        assert!(matches!(error, SourceRuntimeError::Provider { .. }));
    }

    #[test]
    fn every_lx_reference_fixture_should_round_trip_through_catalog_runtime() {
        for (file_name, _) in LX_JS_REFERENCE_SOURCES {
            let report = analyze_lx_js_file(fixture_path(file_name))
                .expect("fixture should analyze before catalog import");
            let provider = ImportedLxManifestProvider::new(report.manifest);
            let runtime = SourceRuntime::new();
            let init_report = runtime
                .initialize_provider(&provider)
                .expect("imported catalog should initialize without JS execution");

            assert!(!init_report.sources.is_empty(), "{file_name}");
            assert!(
                init_report
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.message.contains("Rust provider port")),
                "{file_name}"
            );
        }
    }

    #[test]
    fn qishui_rust_provider_should_search_without_running_js() {
        let api = spawn_qishui_api(1);
        let provider = QishuiRustProvider::with_endpoints(&api, &api, &api);
        let runtime = test_runtime();
        runtime
            .initialize_provider(&provider)
            .expect("qsvip provider should initialize");
        let outcome = runtime
            .dispatch_request(
                &provider,
                SourceRequest::MusicSearch {
                    source: QISHUI_SOURCE_ID.to_owned(),
                    keyword: "晴天".to_owned(),
                    page: 1,
                    page_size: 30,
                },
            )
            .expect("qsvip search should dispatch");

        let SourceResponse::MusicSearch(response) = outcome.response else {
            panic!("qsvip search should return a search response");
        };
        assert_eq!(response.total, Some(1));
        assert_eq!(response.list[0].id, "song-1");
        assert_eq!(response.list[0].title, "晴天");
        assert_eq!(response.list[0].duration_seconds, Some(269));
        assert_eq!(response.list[0].raw_info["songmid"], "song-1");
    }

    #[test]
    fn qishui_rust_provider_should_resolve_music_url_from_search_result() {
        let api = spawn_qishui_api(1);
        let provider = QishuiRustProvider::with_endpoints(&api, &api, &api);
        let runtime = test_runtime();
        runtime
            .initialize_provider(&provider)
            .expect("qsvip provider should initialize");
        let outcome = runtime
            .dispatch_request(
                &provider,
                SourceRequest::MusicUrl {
                    source: QISHUI_SOURCE_ID.to_owned(),
                    quality: SourceQuality::Flac,
                    music_info: serde_json::json!({
                        "id": "song-1",
                        "name": "晴天"
                    }),
                },
            )
            .expect("qsvip musicUrl should dispatch");

        assert_eq!(
            outcome.response,
            SourceResponse::MusicUrl("https://cdn.example.test/song-1.mp3".to_owned())
        );
    }

    #[test]
    fn imported_template_provider_should_resolve_candidate_music_url_without_running_js() {
        let mut report = analyze_lx_js_file(fixture_path("quantouya-aggregate-v4.1.js"))
            .expect("fixture should analyze");
        let endpoint = spawn_http_response(
            "application/json",
            r#"{"code":200,"data":{"url":"https://cdn.example.test/song.mp3"}}"#,
        );
        for template in &mut report.endpoint.templates {
            if template.family == "nianxin" && template.source_id == LX_SOURCE_WY {
                template.url = endpoint.clone();
            }
        }
        let provider = ImportedLxTemplateProvider::from_report(&report, "nianxin")
            .expect("template provider should be created");
        let runtime = test_runtime();
        runtime
            .initialize_provider(&provider)
            .expect("template provider should initialize");
        let outcome = runtime
            .dispatch_request(&provider, mock_music_url_request(LX_SOURCE_WY, "123 456"))
            .expect("template provider should resolve playable URL");

        assert_eq!(
            outcome.response,
            SourceResponse::MusicUrl("https://cdn.example.test/song.mp3".to_owned())
        );
        assert!(outcome
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("playable musicUrl")));
    }

    #[test]
    fn imported_template_provider_should_translate_requested_quality_to_template_level() {
        let mut report = analyze_lx_js_file(fixture_path("quantouya-aggregate-v4.1.js"))
            .expect("fixture should analyze");
        let endpoint = spawn_http_response("text/plain", "https://cdn.example.test/flac.mp3");
        for template in &mut report.endpoint.templates {
            if template.family == "changqing" && template.source_id == LX_SOURCE_TX {
                template.url = endpoint.clone();
            }
        }
        let provider = ImportedLxTemplateProvider::from_report(&report, "changqing")
            .expect("template provider should be created");
        let runtime = test_runtime();
        runtime
            .initialize_provider(&provider)
            .expect("template provider should initialize");
        let outcome = runtime
            .dispatch_request(
                &provider,
                SourceRequest::MusicUrl {
                    source: LX_SOURCE_TX.to_owned(),
                    quality: SourceQuality::Flac,
                    music_info: serde_json::json!({ "songid": 42 }),
                },
            )
            .expect("template provider should build quality candidate URL");

        assert_eq!(
            outcome.response,
            SourceResponse::MusicUrl("https://cdn.example.test/flac.mp3".to_owned())
        );
    }

    #[test]
    fn obfuscated_import_manifest_should_preserve_update_metadata_and_catalog() {
        let report = analyze_lx_js_file(fixture_path("changqing-svip-v1.2.0.js"))
            .expect("fixture should analyze");
        let catalog = report.manifest.to_source_catalog();

        assert_eq!(report.manifest.display_name, "长青SVIP音源");
        assert_eq!(report.manifest.version.as_deref(), Some("1.2.0"));
        assert!(report.manifest.homepage.is_some());
        assert!(report.manifest.update_url.is_some());
        assert!(report.manifest.requires_rust_port);
        assert!(report
            .manifest
            .warnings
            .iter()
            .any(|warning| warning.contains("Rust porting")));
        assert!(catalog.contains_key(LX_SOURCE_KG));
        assert!(catalog[LX_SOURCE_KG]
            .actions
            .contains(&SourceAction::MusicUrl));
        assert!(catalog[LX_SOURCE_KG]
            .qualities
            .contains(&SourceQuality::K128));
    }

    #[test]
    fn obfuscated_sources_should_report_decoder_signals() {
        for file_name in ["nianxin-v1.0.1.js", "changqing-svip-v1.2.0.js"] {
            let report =
                analyze_lx_js_file(fixture_path(file_name)).expect("fixture should analyze");

            assert!(report.obfuscation.likely_obfuscated, "{file_name}");
            assert!(report.obfuscation.hex_identifier_count > 20, "{file_name}");
            assert!(
                report
                    .obfuscation
                    .signals
                    .iter()
                    .any(|signal| signal.contains("RC4-like")),
                "{file_name}"
            );
            assert!(report.contract.requires_deobfuscation_for_full_manifest);
        }
    }

    #[test]
    fn obfuscated_sources_should_decode_core_lx_manifest_strings_without_running_js() {
        for file_name in ["nianxin-v1.0.1.js", "changqing-svip-v1.2.0.js"] {
            let report =
                analyze_lx_js_file(fixture_path(file_name)).expect("fixture should analyze");

            assert!(report.deobfuscation.string_table_len > 200, "{file_name}");
            assert!(report.deobfuscation.decoder_count >= 2, "{file_name}");
            assert!(report.deobfuscation.rotation.is_some(), "{file_name}");
            assert!(
                report
                    .deobfuscation
                    .decoded_strings
                    .iter()
                    .any(|value| value == "request"),
                "{file_name} should decode request"
            );
            assert!(report
                .contract
                .declared_actions
                .contains(&SourceAction::MusicUrl));
            assert!(report
                .contract
                .declared_qualities
                .contains(&SourceQuality::K128));
            assert!(report
                .contract
                .declared_qualities
                .contains(&SourceQuality::K320));
            assert!(report
                .contract
                .declared_qualities
                .contains(&SourceQuality::Flac));
        }
    }

    #[test]
    fn invalid_javascript_should_fail_before_import_report_is_created() {
        let error = analyze_lx_js_source("broken.js", "const = ;")
            .expect_err("invalid JS should fail OXC parsing");

        assert!(matches!(error, LxJsImportError::Parse { .. }));
    }
}
