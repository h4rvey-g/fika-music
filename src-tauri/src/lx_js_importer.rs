use crate::source_runtime::{
    lx_music_source, SourceAction, SourceInfo, SourceQuality, LX_SOURCE_KG, LX_SOURCE_KIND_MUSIC,
    LX_SOURCE_KW, LX_SOURCE_LOCAL, LX_SOURCE_MG, LX_SOURCE_TX, LX_SOURCE_WY,
};
#[cfg(test)]
use crate::source_runtime::{
    SourceCapability, SourceHttpRequest, SourceProvider, SourceRequest, SourceResponse,
    SourceRuntimeContext, SourceRuntimeError, SourceSearchResponse, SourceSearchResult,
};
use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, ArrayExpression, BinaryExpression, Expression, Function, ObjectExpression,
    TemplateLiteral, VariableDeclarator,
};
use oxc_ast::ast_kind::AstKind;
use oxc_ast_visit::Visit;
use oxc_parser::Parser;
use oxc_span::SourceType;
#[cfg(test)]
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
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

#[cfg(test)]
#[derive(Debug, Clone)]
pub struct ImportedLxManifestProvider {
    manifest: ImportedLxManifest,
}

#[cfg(test)]
impl ImportedLxManifestProvider {
    pub fn new(manifest: ImportedLxManifest) -> Self {
        Self { manifest }
    }

    pub fn manifest(&self) -> &ImportedLxManifest {
        &self.manifest
    }
}

#[cfg(test)]
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
            "loaded static LX manifest preview for {}",
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
            "static LX manifest preview {} has no handler for {:?}",
            self.manifest.display_name,
            request.action()
        ));
        Err(context.provider_error("static LX manifest previews are catalog-only"))
    }
}

#[cfg(test)]
const QISHUI_SOURCE_ID: &str = "qsvip";
#[cfg(test)]
const QISHUI_SOURCE_NAME: &str = "汽水VIP";
#[cfg(test)]
const QISHUI_API_HTTPS: &str = "https://api.vsaa.cn/api/music.qishui.vip";
#[cfg(test)]
const QISHUI_API_HTTP: &str = "http://api.vsaa.cn/api/music.qishui.vip";
#[cfg(test)]
const QISHUI_PROXY_API: &str = "https://proxy.qishui.vsaa.cn/qishui/proxy";

#[cfg(test)]
#[derive(Debug, Clone)]
pub struct QishuiRustProvider {
    api_https_url: String,
    api_http_url: String,
    proxy_url: String,
}

#[cfg(test)]
impl Default for QishuiRustProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
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
        let url = append_query_params(url, params)
            .map_err(|error| context.provider_error(format!("汽水VIP请求URL无效: {error}")))?;
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

#[cfg(test)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LxJsImportAdapter {
    QuickJs,
    V8Sidecar,
    Nianxin,
    Changqing,
    StaticTemplates,
}

impl LxJsImportAdapter {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::QuickJs => "quickjs",
            Self::V8Sidecar => "v8-sidecar",
            Self::Nianxin => "nianxin",
            Self::Changqing => "changqing",
            Self::StaticTemplates => "static-templates",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "quickjs" => Some(Self::QuickJs),
            "v8-sidecar" => Some(Self::V8Sidecar),
            "nianxin" => Some(Self::Nianxin),
            "changqing" => Some(Self::Changqing),
            "static-templates" => Some(Self::StaticTemplates),
            _ => None,
        }
    }
}

pub fn supported_import_adapter(report: &LxJsImportReport) -> Option<LxJsImportAdapter> {
    (report.contract.uses_global_lx
        && report.contract.registers_request_handler
        && report
            .contract
            .declared_actions
            .contains(&SourceAction::MusicUrl))
    .then_some(LxJsImportAdapter::QuickJs)
}

#[cfg(test)]
pub fn import_adapter_templates(
    report: &LxJsImportReport,
    adapter: LxJsImportAdapter,
) -> Result<Vec<LxUrlTemplate>, LxJsImportError> {
    let (templates, family) = match adapter {
        LxJsImportAdapter::QuickJs
        | LxJsImportAdapter::V8Sidecar
        | LxJsImportAdapter::StaticTemplates => (report.endpoint.templates.clone(), None),
        LxJsImportAdapter::Nianxin | LxJsImportAdapter::Changqing => {
            (report.endpoint.templates.clone(), Some(adapter.as_str()))
        }
    };
    let templates = templates
        .into_iter()
        .filter(|template| family.is_none_or(|family| template.family == family))
        .filter(|template| template.has_track_id_placeholder)
        .filter(|template| LX_SOURCE_KEYS.contains(&template.source_id.as_str()))
        .collect::<Vec<_>>();
    if templates.is_empty()
        && !matches!(
            adapter,
            LxJsImportAdapter::QuickJs | LxJsImportAdapter::V8Sidecar
        )
    {
        return Err(LxJsImportError::Unsupported(format!(
            "the {} adapter did not expose a track URL template",
            adapter.as_str()
        )));
    }
    Ok(templates)
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub struct ImportedLxTemplateProvider {
    provider_id: String,
    display_name: String,
    adapter_name: String,
    source_catalog: BTreeMap<String, SourceInfo>,
    templates_by_source: BTreeMap<String, Vec<LxUrlTemplate>>,
    aggregate_primary_endpoint: Option<String>,
}

#[cfg(test)]
impl ImportedLxTemplateProvider {
    pub fn from_report(report: &LxJsImportReport, family: &str) -> Result<Self, LxJsImportError> {
        let templates = report
            .endpoint
            .templates
            .iter()
            .filter(|template| template.family == family)
            .cloned()
            .collect();
        Ok(Self::with_templates(
            format!("{}-{family}-template-preview", report.manifest.provider_id),
            report.manifest.display_name.clone(),
            family,
            report.manifest.to_source_catalog(),
            templates,
            None,
        ))
    }

    pub fn for_imported_package(
        provider_id: impl Into<String>,
        report: &LxJsImportReport,
        source_catalog: BTreeMap<String, SourceInfo>,
        adapter: LxJsImportAdapter,
    ) -> Result<Self, LxJsImportError> {
        let templates = import_adapter_templates(report, adapter)?;
        let aggregate_primary_endpoint = (adapter == LxJsImportAdapter::StaticTemplates)
            .then(|| find_aggregate_primary_endpoint(report))
            .flatten();
        Ok(Self::with_templates(
            provider_id,
            report.manifest.display_name.clone(),
            adapter.as_str(),
            source_catalog,
            templates,
            aggregate_primary_endpoint,
        ))
    }

    fn with_templates(
        provider_id: impl Into<String>,
        display_name: impl Into<String>,
        adapter_name: impl Into<String>,
        source_catalog: BTreeMap<String, SourceInfo>,
        templates: Vec<LxUrlTemplate>,
        aggregate_primary_endpoint: Option<String>,
    ) -> Self {
        let mut templates_by_source = BTreeMap::<String, Vec<LxUrlTemplate>>::new();
        for template in templates {
            templates_by_source
                .entry(template.source_id.clone())
                .or_default()
                .push(template);
        }
        Self {
            provider_id: provider_id.into(),
            display_name: display_name.into(),
            adapter_name: adapter_name.into(),
            source_catalog,
            templates_by_source,
            aggregate_primary_endpoint,
        }
    }

    fn templates_for_source(&self, source: &str) -> Option<&[LxUrlTemplate]> {
        self.templates_by_source.get(source).map(Vec::as_slice)
    }

    fn resolved_template_url(
        &self,
        context: &mut SourceRuntimeContext,
        endpoint_url: &str,
    ) -> Result<String, SourceRuntimeError> {
        resolve_template_endpoint(context, endpoint_url)
    }
}

#[cfg(test)]
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
            "loaded test-only {} URL-template preview for {}",
            self.adapter_name, self.display_name
        ));
        Ok(self.source_catalog.clone())
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

        let Some(templates) = self.templates_for_source(&source) else {
            return Err(context.provider_error(format!(
                "no {} URL template candidate for source {}",
                self.adapter_name, source
            )));
        };
        let Some(track_id) = extract_track_id(&music_info) else {
            return Err(context.provider_error("musicUrl request is missing a track id"));
        };

        let level = quality_to_template_level(quality);
        let mut failures = Vec::new();
        if let Some(endpoint_url) = self.aggregate_primary_endpoint.as_deref() {
            match render_aggregate_primary_url(endpoint_url, &source, &track_id, quality) {
                Ok(Some(url)) => match resolve_aggregate_endpoint(context, &url) {
                    Ok(resolved_url) => {
                        context
                            .info("resolved playable musicUrl via imported aggregate primary API");
                        return Ok(SourceResponse::MusicUrl(resolved_url));
                    }
                    Err(error @ SourceRuntimeError::Cancelled { .. }) => return Err(error),
                    Err(error) => failures.push(format!("aggregate-primary: {error}")),
                },
                Ok(None) => {}
                Err(error) => failures.push(format!("aggregate-primary: {error}")),
            }
        }
        for template in templates {
            let url = match render_template_url(&template.url, &track_id, level) {
                Ok(url) => url,
                Err(error) => {
                    failures.push(format!("{}: {error}", template.family));
                    continue;
                }
            };
            match self.resolved_template_url(context, &url) {
                Ok(resolved_url) => {
                    context.info(format!(
                        "resolved playable musicUrl via imported {} Rust template provider",
                        template.family
                    ));
                    return Ok(SourceResponse::MusicUrl(resolved_url));
                }
                Err(error @ SourceRuntimeError::Cancelled { .. }) => return Err(error),
                Err(error) => failures.push(format!("{}: {error}", template.family)),
            }
        }

        Err(context.provider_error(format!(
            "all {} music URL candidates failed: {}",
            self.adapter_name,
            failures.join("; ")
        )))
    }
}

/*
 * The network client belongs to SourceRuntime's host service. Keeping endpoint
 * resolution here as a pure response normalizer prevents imported sources from
 * acquiring a second, unreviewed HTTP boundary.
 */
#[cfg(test)]
fn resolve_template_endpoint(
    context: &mut SourceRuntimeContext,
    endpoint_url: &str,
) -> Result<String, SourceRuntimeError> {
    resolve_music_url_endpoint(
        context,
        endpoint_url,
        "request imported template endpoint",
        "template endpoint",
    )
}

#[cfg(test)]
fn resolve_aggregate_endpoint(
    context: &mut SourceRuntimeContext,
    endpoint_url: &str,
) -> Result<String, SourceRuntimeError> {
    resolve_music_url_endpoint(
        context,
        endpoint_url,
        "request imported aggregate endpoint",
        "aggregate endpoint",
    )
}

#[cfg(test)]
fn resolve_music_url_endpoint(
    context: &mut SourceRuntimeContext,
    endpoint_url: &str,
    request_operation: &'static str,
    endpoint_name: &'static str,
) -> Result<String, SourceRuntimeError> {
    let response = context.http_request(SourceHttpRequest::get(endpoint_url), request_operation)?;

    if !response.is_success() {
        return Err(context.provider_error(format!(
            "{endpoint_name} {endpoint_url} returned HTTP {}",
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
    let body = body.trim().trim_matches('"');
    if is_http_url(body) {
        return Ok(body.to_owned());
    }
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(url) = find_url_in_json(&json) {
            return Ok(url);
        }
    }

    Err(context.provider_error(format!("{endpoint_name} did not return a playable URL")))
}

#[cfg(test)]
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

#[cfg(test)]
fn is_http_url(value: &str) -> bool {
    url::Url::parse(value)
        .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
}

#[cfg(test)]
fn append_query_params(url: &str, params: &[(&str, String)]) -> Result<String, url::ParseError> {
    let mut url = parse_http_url(url)?;
    if !params.is_empty() {
        url.query_pairs_mut()
            .extend_pairs(params.iter().map(|(key, value)| (*key, value.as_str())));
    }
    Ok(url.into())
}

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
fn normalize_qishui_quality(quality: SourceQuality) -> &'static str {
    match quality {
        SourceQuality::K128 => "low",
        SourceQuality::K320 => "standard",
        SourceQuality::Flac => "lossless",
        SourceQuality::Flac24Bit => "flac24bit",
    }
}

#[cfg(test)]
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
        track_number: None,
        disc_number: None,
        platform_ids: BTreeMap::new(),
        raw_info,
    }
}

#[cfg(test)]
fn json_string(value: Option<&serde_json::Value>) -> Option<String> {
    match value? {
        serde_json::Value::String(value) => Some(value.clone()),
        serde_json::Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

#[cfg(test)]
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

#[derive(Debug, Clone, PartialEq)]
enum AstValue {
    String(String),
    Number(usize),
    Path(String),
    Other,
}

#[derive(Debug, Clone, PartialEq)]
struct AstCall {
    callee: String,
    arguments: Vec<AstValue>,
}

#[derive(Debug, Clone, Default)]
struct JsAstFacts {
    hex_identifier_count: usize,
    symbols: BTreeSet<String>,
    member_paths: BTreeSet<String>,
    string_literals: Vec<String>,
    string_arrays: Vec<Vec<String>>,
    numeric_literals: BTreeSet<usize>,
    aliases: BTreeMap<String, String>,
    calls: Vec<AstCall>,
    decoder_specs: Vec<StringDecoderSpec>,
    url_templates: Vec<LxUrlTemplate>,
}

impl JsAstFacts {
    fn has_symbol(&self, value: &str) -> bool {
        self.symbols.contains(value)
    }

    fn has_member_path(&self, value: &str) -> bool {
        self.member_paths.contains(value)
    }

    fn has_call(&self, callee: &str) -> bool {
        self.calls.iter().any(|call| call.callee == callee)
    }

    fn has_call_with_first_argument(&self, callee: &str, argument: &str) -> bool {
        self.calls.iter().any(|call| {
            call.callee == callee
                && call
                    .arguments
                    .first()
                    .is_some_and(|value| value.matches(argument))
        })
    }

    fn has_string(&self, value: &str) -> bool {
        self.string_literals.iter().any(|item| item == value)
    }
}

impl AstValue {
    fn matches(&self, expected: &str) -> bool {
        matches!(self, Self::String(value) | Self::Path(value) if value == expected)
    }
}

#[derive(Default)]
struct JsAstCollector {
    facts: JsAstFacts,
}

impl JsAstCollector {
    fn into_facts(self) -> JsAstFacts {
        self.facts
    }

    fn push_string(&mut self, value: &str) {
        self.facts.symbols.insert(value.to_owned());
        if !self.facts.string_literals.iter().any(|item| item == value) {
            self.facts.string_literals.push(value.to_owned());
        }
    }

    fn collect_variable_declarator(&mut self, declarator: &VariableDeclarator<'_>) {
        let Some(identifier) = declarator.id.get_binding_identifier() else {
            return;
        };
        let name = identifier.name.as_str();
        self.facts.symbols.insert(name.to_owned());
        let Some(initializer) = declarator.init.as_ref() else {
            return;
        };

        if let Expression::Identifier(alias) = initializer {
            self.facts
                .aliases
                .insert(name.to_owned(), alias.name.to_string());
        }

        let Some(family) = name
            .strip_suffix("URL_TEMPLATES")
            .map(|value| value.trim_end_matches('_').to_ascii_lowercase())
        else {
            return;
        };
        let Expression::ObjectExpression(object) = initializer else {
            return;
        };
        self.facts
            .url_templates
            .extend(url_templates_from_object(&family, object));
    }

    fn collect_call(&mut self, call: &oxc_ast::ast::CallExpression<'_>) {
        let Some(callee) = expression_path(&call.callee) else {
            return;
        };
        self.facts.calls.push(AstCall {
            callee,
            arguments: call.arguments.iter().map(ast_value_from_argument).collect(),
        });
    }

    fn collect_function(&mut self, function: &Function<'_>) {
        let Some(name) = function
            .id
            .as_ref()
            .map(|identifier| identifier.name.as_str())
        else {
            return;
        };
        if !name.starts_with("_0x") {
            return;
        }
        let Some(body) = function.body.as_ref() else {
            return;
        };
        let mut inspector = DecoderFunctionInspector::default();
        inspector.visit_function_body(body);
        let Some(offset) = inspector.offsets.first().copied() else {
            return;
        };
        let kind = if inspector.numeric_literals.contains(&256)
            && inspector.symbols.contains("charCodeAt")
            && inspector.symbols.contains("fromCharCode")
        {
            StringDecoderKind::Rc4
        } else if inspector.symbols.contains("decodeURIComponent") {
            StringDecoderKind::Base64Percent
        } else {
            return;
        };
        self.facts.decoder_specs.push(StringDecoderSpec {
            name: name.to_owned(),
            offset,
            kind,
        });
    }
}

impl<'a> Visit<'a> for JsAstCollector {
    fn enter_node(&mut self, kind: AstKind<'a>) {
        match kind {
            AstKind::IdentifierName(identifier) => {
                self.facts.symbols.insert(identifier.name.to_string());
                if identifier.name.starts_with("_0x") {
                    self.facts.hex_identifier_count += 1;
                }
            }
            AstKind::IdentifierReference(identifier) => {
                self.facts.symbols.insert(identifier.name.to_string());
                if identifier.name.starts_with("_0x") {
                    self.facts.hex_identifier_count += 1;
                }
            }
            AstKind::BindingIdentifier(identifier) => {
                self.facts.symbols.insert(identifier.name.to_string());
                if identifier.name.starts_with("_0x") {
                    self.facts.hex_identifier_count += 1;
                }
            }
            AstKind::StringLiteral(literal) => self.push_string(literal.value.as_str()),
            AstKind::TemplateLiteral(template) => {
                if let Some(value) = template_literal_text(template) {
                    self.push_string(&value);
                }
            }
            AstKind::NumericLiteral(literal) => {
                if let Some(value) = number_to_usize(literal.value) {
                    self.facts.numeric_literals.insert(value);
                }
            }
            AstKind::BinaryExpression(expression) => {
                if constant_binary_number(expression).and_then(number_to_usize) == Some(256) {
                    self.facts.numeric_literals.insert(256);
                }
            }
            AstKind::StaticMemberExpression(member) => {
                if let Some(path) =
                    static_member_path(&member.object, member.property.name.as_str())
                {
                    self.facts.member_paths.insert(path);
                }
            }
            AstKind::ComputedMemberExpression(member) => {
                if let Some(path) = computed_member_path(&member.object, &member.expression) {
                    self.facts.member_paths.insert(path);
                }
            }
            AstKind::ArrayExpression(array) => {
                if let Some(values) = string_array(array) {
                    self.facts.string_arrays.push(values);
                }
            }
            AstKind::VariableDeclarator(declarator) => {
                self.collect_variable_declarator(declarator);
            }
            AstKind::CallExpression(call) => self.collect_call(call),
            AstKind::Function(function) => self.collect_function(function),
            _ => {}
        }
    }
}

#[derive(Default)]
struct DecoderFunctionInspector {
    symbols: BTreeSet<String>,
    numeric_literals: BTreeSet<usize>,
    offsets: Vec<usize>,
}

impl<'a> Visit<'a> for DecoderFunctionInspector {
    fn enter_node(&mut self, kind: AstKind<'a>) {
        match kind {
            AstKind::IdentifierName(identifier) => {
                self.symbols.insert(identifier.name.to_string());
            }
            AstKind::IdentifierReference(identifier) => {
                self.symbols.insert(identifier.name.to_string());
            }
            AstKind::StringLiteral(literal) => {
                self.symbols.insert(literal.value.to_string());
            }
            AstKind::NumericLiteral(literal) => {
                if let Some(value) = number_to_usize(literal.value) {
                    self.numeric_literals.insert(value);
                }
            }
            AstKind::BinaryExpression(expression) => {
                if let Some(value) = constant_binary_number(expression).and_then(number_to_usize) {
                    self.numeric_literals.insert(value);
                }
                if expression.operator == oxc_ast::ast::BinaryOperator::Subtraction
                    && matches!(expression.left, Expression::Identifier(_))
                {
                    if let Some(value) =
                        constant_number(&expression.right).and_then(number_to_usize)
                    {
                        self.offsets.push(value);
                    }
                }
            }
            _ => {}
        }
    }
}

fn expression_path(expression: &Expression<'_>) -> Option<String> {
    match expression {
        Expression::Identifier(identifier) => Some(identifier.name.to_string()),
        Expression::StaticMemberExpression(member) => {
            static_member_path(&member.object, member.property.name.as_str())
        }
        Expression::ComputedMemberExpression(member) => {
            computed_member_path(&member.object, &member.expression)
        }
        Expression::ParenthesizedExpression(expression) => expression_path(&expression.expression),
        _ => None,
    }
}

fn static_member_path(object: &Expression<'_>, property: &str) -> Option<String> {
    let object = expression_path(object)?;
    Some(format!("{object}.{property}"))
}

fn computed_member_path(object: &Expression<'_>, property: &Expression<'_>) -> Option<String> {
    let object = expression_path(object)?;
    let property = static_expression_text(property)?;
    Some(format!("{object}.{property}"))
}

fn static_expression_text(expression: &Expression<'_>) -> Option<String> {
    match expression {
        Expression::StringLiteral(literal) => Some(literal.value.to_string()),
        Expression::TemplateLiteral(template) => {
            template.single_quasi().map(|value| value.to_string())
        }
        Expression::Identifier(identifier) => Some(identifier.name.to_string()),
        _ => None,
    }
}

fn ast_value_from_argument(argument: &Argument<'_>) -> AstValue {
    argument
        .as_expression()
        .map(ast_value_from_expression)
        .unwrap_or(AstValue::Other)
}

fn ast_value_from_expression(expression: &Expression<'_>) -> AstValue {
    match expression {
        Expression::StringLiteral(literal) => AstValue::String(literal.value.to_string()),
        Expression::TemplateLiteral(template) => template_literal_text(template)
            .map(AstValue::String)
            .unwrap_or(AstValue::Other),
        Expression::NumericLiteral(literal) => number_to_usize(literal.value)
            .map(AstValue::Number)
            .unwrap_or(AstValue::Other),
        _ => constant_number(expression)
            .and_then(number_to_usize)
            .map(AstValue::Number)
            .or_else(|| expression_path(expression).map(AstValue::Path))
            .unwrap_or(AstValue::Other),
    }
}

fn constant_number(expression: &Expression<'_>) -> Option<f64> {
    let value = match expression {
        Expression::NumericLiteral(literal) => literal.value,
        Expression::UnaryExpression(expression) => {
            let argument = constant_number(&expression.argument)?;
            match expression.operator {
                oxc_ast::ast::UnaryOperator::UnaryPlus => argument,
                oxc_ast::ast::UnaryOperator::UnaryNegation => -argument,
                _ => return None,
            }
        }
        Expression::BinaryExpression(expression) => constant_binary_number(expression)?,
        Expression::ParenthesizedExpression(expression) => constant_number(&expression.expression)?,
        _ => return None,
    };
    value.is_finite().then_some(value)
}

fn constant_binary_number(expression: &BinaryExpression<'_>) -> Option<f64> {
    let left = constant_number(&expression.left)?;
    let right = constant_number(&expression.right)?;
    let value = match expression.operator {
        oxc_ast::ast::BinaryOperator::Addition => left + right,
        oxc_ast::ast::BinaryOperator::Subtraction => left - right,
        oxc_ast::ast::BinaryOperator::Multiplication => left * right,
        oxc_ast::ast::BinaryOperator::Division if right != 0.0 => left / right,
        oxc_ast::ast::BinaryOperator::Remainder if right != 0.0 => left % right,
        oxc_ast::ast::BinaryOperator::Exponential => left.powf(right),
        _ => return None,
    };
    value.is_finite().then_some(value)
}

fn number_to_usize(value: f64) -> Option<usize> {
    if value.is_finite() && value >= 0.0 && value.fract() == 0.0 && value <= usize::MAX as f64 {
        Some(value as usize)
    } else {
        None
    }
}

fn template_literal_text(template: &TemplateLiteral<'_>) -> Option<String> {
    let mut value = String::new();
    for (index, quasi) in template.quasis.iter().enumerate() {
        value.push_str(quasi.value.cooked.unwrap_or(quasi.value.raw).as_str());
        if let Some(expression) = template.expressions.get(index) {
            value.push_str("${");
            value.push_str(&expression_path(expression)?);
            value.push('}');
        }
    }
    Some(value)
}

fn string_array(array: &ArrayExpression<'_>) -> Option<Vec<String>> {
    array
        .elements
        .iter()
        .map(|element| {
            let expression = element.as_expression()?;
            match expression {
                Expression::StringLiteral(literal) => Some(literal.value.to_string()),
                Expression::TemplateLiteral(template) => {
                    template.single_quasi().map(|value| value.to_string())
                }
                _ => None,
            }
        })
        .collect()
}

fn url_templates_from_object(family: &str, object: &ObjectExpression<'_>) -> Vec<LxUrlTemplate> {
    object
        .properties
        .iter()
        .filter_map(|property| property.as_property())
        .filter_map(|property| {
            let source_id = property.key.static_name()?.into_owned();
            if !LX_SOURCE_KEYS.contains(&source_id.as_str()) {
                return None;
            }
            let url = match &property.value {
                Expression::StringLiteral(literal) => literal.value.to_string(),
                Expression::TemplateLiteral(template) => template_literal_text(template)?,
                _ => return None,
            };
            is_url_template(&url).then(|| LxUrlTemplate {
                family: family.to_owned(),
                source_id,
                domain: extract_domain(&url),
                has_track_id_placeholder: url.contains("{id}") || url.contains("${id}"),
                has_quality_placeholder: { url.contains("{level}") || url.contains("${level}") },
                url,
            })
        })
        .collect()
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
    #[error("LX JS source is not safely importable: {0}")]
    Unsupported(String),
}

struct ParsedJs {
    report: JsParseReport,
    facts: JsAstFacts,
    comments: Vec<String>,
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
    let parsed = parse_with_oxc(path, source)?;
    let obfuscation = scan_obfuscation(source, &parsed.facts);
    let metadata = parse_metadata(&parsed.comments);
    let deobfuscation = deobfuscate_string_literals(&parsed.facts, &metadata);
    let contract = scan_lx_contract(&parsed.facts, &deobfuscation, obfuscation.likely_obfuscated);
    let endpoint = scan_endpoint_report(&parsed.facts, &deobfuscation.decoded_strings);
    let manifest = build_import_manifest(&metadata, &contract, &obfuscation, &deobfuscation);

    Ok(LxJsImportReport {
        file_name: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("<memory>")
            .to_owned(),
        metadata,
        parse: parsed.report,
        contract,
        endpoint,
        obfuscation,
        deobfuscation,
        manifest,
    })
}

fn parse_with_oxc(path: &Path, source: &str) -> Result<ParsedJs, LxJsImportError> {
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

    let comments = parser_return
        .program
        .comments
        .iter()
        .map(|comment| comment.content_span().source_text(source).to_owned())
        .collect();
    let mut collector = JsAstCollector::default();
    collector.visit_program(&parser_return.program);

    Ok(ParsedJs {
        report: JsParseReport {
            parsed_with_oxc: true,
            top_level_statement_count: parser_return.program.body.len(),
            comment_count: parser_return.program.comments.len(),
            diagnostics,
        },
        facts: collector.into_facts(),
        comments,
    })
}

fn parse_metadata(comments: &[String]) -> LxJsMetadata {
    let mut metadata = LxJsMetadata::default();

    for line in comments.iter().flat_map(|comment| comment.lines()) {
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
    facts: &JsAstFacts,
    deobfuscation: &LxJsDeobfuscationReport,
    likely_obfuscated: bool,
) -> LxJsContractReport {
    let declared_actions = scan_declared_actions(facts, &deobfuscation.decoded_strings);
    let declared_qualities = scan_declared_qualities(facts, &deobfuscation.decoded_strings);
    let declared_sources = scan_declared_sources(facts, &declared_actions, &declared_qualities);
    let references_qualitys = facts.has_symbol("qualitys") || facts.has_symbol("qualities");
    let uses_event_names = facts.has_symbol("EVENT_NAMES");

    LxJsContractReport {
        uses_global_lx: facts.has_member_path("globalThis.lx"),
        uses_event_names,
        uses_lx_request: facts.has_symbol("request"),
        registers_request_handler: uses_event_names
            && (facts.has_call_with_first_argument("on", "EVENT_NAMES.request")
                || facts.has_call("on")),
        sends_any_lx_event: uses_event_names && facts.has_call("send"),
        sends_inited_event_literal: facts.has_member_path("EVENT_NAMES.inited")
            || facts.has_string("inited")
            || deobfuscation
                .decoded_strings
                .iter()
                .any(|value| value == "inited"),
        sends_update_alert_event_literal: facts.has_member_path("EVENT_NAMES.updateAlert")
            || facts.has_string("updateAlert")
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
        warnings.push(
            "static source details may be incomplete; runtime initialization verifies the LX contract"
                .to_owned(),
        );
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
        requires_rust_port: false,
        warnings,
    }
}

fn provider_id_from_metadata(metadata: &LxJsMetadata) -> String {
    const MAX_PROVIDER_SLUG_BYTES: usize = 48;

    let seed = metadata
        .name
        .as_deref()
        .or(metadata.author.as_deref())
        .unwrap_or("imported-lx-source");
    let mut normalized = String::new();
    let mut previous_dash = false;

    for character in seed.chars() {
        if normalized.len() >= MAX_PROVIDER_SLUG_BYTES {
            break;
        }
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

#[cfg(test)]
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

#[cfg(test)]
fn quality_to_template_level(quality: SourceQuality) -> &'static str {
    match quality {
        SourceQuality::Flac | SourceQuality::Flac24Bit => "lossless",
        SourceQuality::K320 => "exhigh",
        SourceQuality::K128 => "standard",
    }
}

#[cfg(test)]
fn find_aggregate_primary_endpoint(report: &LxJsImportReport) -> Option<String> {
    report.endpoint.urls.iter().find_map(|candidate| {
        let url = parse_http_url(candidate).ok()?;
        let has_xbridge_marker = url
            .query_pairs()
            .any(|(key, value)| key == "use_xbridge3" && value == "true");
        (url.path().ends_with("/api.php") && has_xbridge_marker).then(|| candidate.clone())
    })
}

#[cfg(test)]
fn render_aggregate_primary_url(
    endpoint: &str,
    source: &str,
    track_id: &str,
    quality: SourceQuality,
) -> Result<Option<String>, url::ParseError> {
    let Some(source_name) = aggregate_source_name(source) else {
        return Ok(None);
    };
    let mut url = parse_http_url(endpoint)?;
    url.query_pairs_mut()
        .append_pair("types", "url")
        .append_pair("source", source_name)
        .append_pair("id", track_id)
        .append_pair("br", aggregate_quality_bitrate(quality));
    Ok(Some(url.into()))
}

#[cfg(test)]
fn aggregate_source_name(source: &str) -> Option<&'static str> {
    match source {
        LX_SOURCE_WY => Some("netease"),
        LX_SOURCE_TX => Some("tencent"),
        LX_SOURCE_KW => Some("kuwo"),
        LX_SOURCE_KG => Some("kugou"),
        LX_SOURCE_MG => Some("migu"),
        _ => None,
    }
}

#[cfg(test)]
fn aggregate_quality_bitrate(quality: SourceQuality) -> &'static str {
    match quality {
        SourceQuality::K128 => "128",
        SourceQuality::K320 => "320",
        SourceQuality::Flac => "740",
        SourceQuality::Flac24Bit => "999",
    }
}

#[cfg(test)]
fn render_template_url(
    template: &str,
    track_id: &str,
    level: &str,
) -> Result<String, url::ParseError> {
    const COMPONENT_ENCODE_SET: &percent_encoding::AsciiSet = &NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'_')
        .remove(b'.')
        .remove(b'~');

    let track_id = utf8_percent_encode(track_id, COMPONENT_ENCODE_SET).to_string();
    let level = utf8_percent_encode(level, COMPONENT_ENCODE_SET).to_string();
    let rendered = template
        .replace("${id}", &track_id)
        .replace("{id}", &track_id)
        .replace("${level}", &level)
        .replace("{level}", &level);
    parse_http_url(&rendered).map(Into::into)
}

#[cfg(test)]
fn parse_http_url(value: &str) -> Result<url::Url, url::ParseError> {
    let url = url::Url::parse(value)?;
    if matches!(url.scheme(), "http" | "https") && url.host_str().is_some() {
        Ok(url)
    } else {
        Err(url::ParseError::RelativeUrlWithoutBase)
    }
}

fn scan_declared_actions(facts: &JsAstFacts, decoded_strings: &[String]) -> Vec<SourceAction> {
    let mut actions = Vec::new();
    if facts.has_symbol("musicSearch")
        || facts.has_string("search")
        || decoded_strings
            .iter()
            .any(|value| value == "musicSearch" || value == "search")
    {
        push_unique_action(&mut actions, SourceAction::MusicSearch);
    }
    if facts.has_symbol("musicUrl") || decoded_strings.iter().any(|value| value == "musicUrl") {
        push_unique_action(&mut actions, SourceAction::MusicUrl);
    }
    if facts.has_symbol("lyric") || decoded_strings.iter().any(|value| value == "lyric") {
        push_unique_action(&mut actions, SourceAction::Lyric);
    }
    if facts.has_symbol("pic") || decoded_strings.iter().any(|value| value == "pic") {
        push_unique_action(&mut actions, SourceAction::Pic);
    }
    actions
}

fn scan_declared_qualities(facts: &JsAstFacts, decoded_strings: &[String]) -> Vec<SourceQuality> {
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
        if facts.has_string(needle)
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
    facts: &JsAstFacts,
    declared_actions: &[SourceAction],
    declared_qualities: &[SourceQuality],
) -> BTreeMap<String, ImportedLxSourceInfo> {
    let mut sources = BTreeMap::new();
    for source_id in LX_SOURCE_KEYS {
        if facts.has_symbol(source_id) {
            let qualities = if *source_id == LX_SOURCE_LOCAL {
                Vec::new()
            } else {
                declared_qualities.to_vec()
            };
            sources.insert(
                (*source_id).to_owned(),
                ImportedLxSourceInfo {
                    id: (*source_id).to_owned(),
                    name: infer_source_display_name(facts, source_id),
                    kind: LX_SOURCE_KIND_MUSIC.to_owned(),
                    actions: declared_actions.to_vec(),
                    qualities,
                },
            );
        }
    }
    sources
}

fn infer_source_display_name(facts: &JsAstFacts, source_id: &str) -> String {
    for (id, display_name) in [
        (LX_SOURCE_WY, "网易云音乐"),
        (LX_SOURCE_TX, "QQ音乐"),
        (LX_SOURCE_KW, "酷我音乐"),
        (LX_SOURCE_KG, "酷狗音乐"),
        (LX_SOURCE_MG, "咪咕音乐"),
        (LX_SOURCE_LOCAL, "本地音乐"),
    ] {
        if source_id == id && facts.has_string(display_name) {
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

fn scan_endpoint_report(facts: &JsAstFacts, decoded_strings: &[String]) -> LxJsEndpointReport {
    let mut urls = Vec::new();
    for literal in &facts.string_literals {
        collect_urls_from_text(literal, &mut urls);
    }
    for decoded in decoded_strings {
        collect_urls_from_text(decoded, &mut urls);
    }

    let templates = facts.url_templates.clone();
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

fn is_url_template(url: &str) -> bool {
    url.contains("{id}") || url.contains("{level}") || url.contains("${")
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
    url::Url::parse(url)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .and_then(|url| url.host_str().map(str::to_owned))
}

fn scan_obfuscation(source: &str, facts: &JsAstFacts) -> LxJsObfuscationReport {
    let hex_identifier_count = facts.hex_identifier_count;
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
    if facts
        .member_paths
        .iter()
        .any(|path| path.ends_with(".push"))
        && facts
            .member_paths
            .iter()
            .any(|path| path.ends_with(".shift"))
    {
        signals.push("string-array rotation loop".to_owned());
    }
    if facts.has_string("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789+/=")
        && facts.has_symbol("decodeURIComponent")
    {
        signals.push("base64 percent-decoder string table".to_owned());
    }
    if facts.numeric_literals.contains(&256)
        && facts.has_symbol("charCodeAt")
        && facts.has_symbol("fromCharCode")
    {
        signals.push("RC4-like string decoder".to_owned());
    }
    if facts.has_symbol("qualitys") && scan_declared_qualities(facts, &[]).is_empty() {
        signals.push("quality labels are not present as plain string literals".to_owned());
    }

    LxJsObfuscationReport {
        likely_obfuscated: !signals.is_empty(),
        hex_identifier_count,
        long_line_count,
        signals,
    }
}

fn deobfuscate_string_literals(
    facts: &JsAstFacts,
    metadata: &LxJsMetadata,
) -> LxJsDeobfuscationReport {
    let string_table = facts
        .string_arrays
        .iter()
        .max_by_key(|values| values.len())
        .cloned()
        .unwrap_or_default();
    let decoder_specs = &facts.decoder_specs;
    if string_table.is_empty() || decoder_specs.is_empty() {
        return LxJsDeobfuscationReport {
            string_table_len: string_table.len(),
            decoder_count: decoder_specs.len(),
            ..LxJsDeobfuscationReport::default()
        };
    }

    let aliases = resolve_decoder_aliases(facts, decoder_specs);
    let calls = decoder_calls_from_ast(facts, &aliases);
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

fn resolve_decoder_aliases(
    facts: &JsAstFacts,
    decoder_specs: &[StringDecoderSpec],
) -> BTreeMap<String, StringDecoderSpec> {
    let mut aliases = decoder_specs
        .iter()
        .map(|spec| (spec.name.clone(), spec.clone()))
        .collect::<BTreeMap<_, _>>();

    loop {
        let mut changed = false;
        for (alias, target) in &facts.aliases {
            let Some(spec) = aliases.get(target).cloned() else {
                continue;
            };
            if aliases.insert(alias.clone(), spec).is_none() {
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    aliases
}

fn decoder_calls_from_ast(
    facts: &JsAstFacts,
    aliases: &BTreeMap<String, StringDecoderSpec>,
) -> Vec<DecoderCall> {
    facts
        .calls
        .iter()
        .filter_map(|call| {
            let decoder = aliases.get(&call.callee)?.clone();
            let index = decoder_index(call.arguments.first()?)?;
            let key = call.arguments.get(1).and_then(|value| match value {
                AstValue::String(value) => Some(value.clone()),
                _ => None,
            });
            if decoder.kind == StringDecoderKind::Rc4 && key.is_none() {
                return None;
            }
            Some(DecoderCall {
                decoder,
                index,
                key,
            })
        })
        .collect()
}

fn decoder_index(value: &AstValue) -> Option<usize> {
    match value {
        AstValue::Number(value) => Some(*value),
        AstValue::String(value) => parse_static_usize(value),
        AstValue::Path(_) | AstValue::Other => None,
    }
}

fn parse_static_usize(value: &str) -> Option<usize> {
    let value = value.trim();
    if let Some(value) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        usize::from_str_radix(value, 16).ok()
    } else if let Some(value) = value
        .strip_prefix("0o")
        .or_else(|| value.strip_prefix("0O"))
    {
        usize::from_str_radix(value, 8).ok()
    } else if let Some(value) = value
        .strip_prefix("0b")
        .or_else(|| value.strip_prefix("0B"))
    {
        usize::from_str_radix(value, 2).ok()
    } else {
        value.parse().ok()
    }
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
        mock_music_url_request, DefaultSourceHost, SourceCancellationToken, SourceCapability,
        SourceHost, SourceHostError, SourceHttpRequest, SourceHttpResponse, SourceKind,
        SourceRuntime,
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

    #[derive(Debug)]
    struct AggregateFallbackHost;

    impl SourceHost for AggregateFallbackHost {
        fn http_request(
            &self,
            _source_id: &str,
            request: &SourceHttpRequest,
            _cancellation: &SourceCancellationToken,
        ) -> Result<SourceHttpResponse, SourceHostError> {
            let request_url = url::Url::parse(&request.url).map_err(|error| {
                SourceHostError::InvalidResponse {
                    message: error.to_string(),
                }
            })?;
            let query = request_url.query_pairs().collect::<BTreeMap<_, _>>();
            let is_expected_aggregate_request = request_url.host_str()
                == Some("aggregate.example.test")
                && query.get("types").is_some_and(|value| value == "url")
                && query.get("source").is_some_and(|value| value == "netease")
                && query.get("id").is_some_and(|value| value == "3402250883")
                && query.get("br").is_some_and(|value| value == "740");
            let (status, content_type, body) = if is_expected_aggregate_request {
                (
                    200,
                    Some("application/json".to_owned()),
                    br#"{"url":"https://cdn.example.test/track.flac"}"#.to_vec(),
                )
            } else {
                (404, Some("text/plain".to_owned()), Vec::new())
            };
            Ok(SourceHttpResponse {
                status,
                final_url: request.url.clone(),
                headers: BTreeMap::new(),
                content_type,
                body,
            })
        }
    }

    fn fixture_path(file_name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/lx-js-sources")
            .join(file_name)
    }

    #[test]
    fn arithmetic_obfuscated_decoder_should_fold_static_indices() {
        let report = analyze_lx_js_file(fixture_path("arithmetic-obfuscated-v1.0.0.js"))
            .expect("arithmetic-obfuscated source should analyze");
        let core_strings = report
            .deobfuscation
            .decoded_strings
            .iter()
            .filter(|value| matches!(value.as_str(), "request" | "musicUrl" | "inited" | "128k"))
            .cloned()
            .collect::<Vec<_>>();

        assert_eq!(
            (
                report.deobfuscation.decoder_count,
                report.deobfuscation.rotation,
                core_strings,
            ),
            (
                1,
                Some(0),
                vec![
                    "request".to_owned(),
                    "musicUrl".to_owned(),
                    "inited".to_owned(),
                    "128k".to_owned(),
                ],
            )
        );
    }

    #[test]
    fn arithmetic_obfuscated_source_should_expose_music_url_contract() {
        let report = analyze_lx_js_file(fixture_path("arithmetic-obfuscated-v1.0.0.js"))
            .expect("arithmetic-obfuscated source should analyze");
        let source = report
            .manifest
            .sources
            .get(LX_SOURCE_KG)
            .expect("Kugou source should be inferred");

        assert_eq!(
            supported_import_adapter(&report),
            Some(LxJsImportAdapter::QuickJs)
        );
        assert_eq!(source.actions, [SourceAction::MusicUrl]);
        assert_eq!(source.qualities, [SourceQuality::K128]);
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
        assert!(!report.manifest.requires_rust_port);
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
    fn static_manifest_preview_should_initialize_catalog_but_reject_requests() {
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
            .any(|diagnostic| diagnostic.message.contains("static LX manifest preview")));

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
                .expect("static catalog preview should initialize");

            assert!(!init_report.sources.is_empty(), "{file_name}");
            assert!(
                init_report
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.message.contains("static LX manifest preview")),
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
    fn imported_static_template_provider_should_prefer_aggregate_primary_api() {
        let mut report = analyze_lx_js_file(fixture_path("quantouya-aggregate-v4.1.js"))
            .expect("fixture should analyze");
        let primary_endpoint = report
            .endpoint
            .urls
            .iter_mut()
            .find(|url| url.contains("music-api.gdstudio.xyz"))
            .expect("aggregate fixture should expose its primary endpoint");
        *primary_endpoint = "https://aggregate.example.test/api.php?use_xbridge3=true".to_owned();
        for template in &mut report.endpoint.templates {
            if template.source_id == LX_SOURCE_WY {
                template.url = format!(
                    "https://templates.example.test/{}/{{id}}?level={{level}}",
                    template.family
                );
            }
        }
        let provider = ImportedLxTemplateProvider::for_imported_package(
            "aggregate-provider",
            &report,
            report.manifest.to_source_catalog(),
            LxJsImportAdapter::StaticTemplates,
        )
        .expect("aggregate template provider should be created");
        let runtime = SourceRuntime::with_host(
            Arc::new(AggregateFallbackHost),
            [SourceCapability::NetworkAny],
        );
        runtime
            .initialize_provider(&provider)
            .expect("aggregate provider should initialize");

        let outcome = runtime
            .dispatch_request(
                &provider,
                SourceRequest::MusicUrl {
                    source: LX_SOURCE_WY.to_owned(),
                    quality: SourceQuality::Flac,
                    music_info: serde_json::json!({ "id": "3402250883" }),
                },
            )
            .expect("aggregate primary API should resolve before stale templates");

        assert_eq!(
            outcome.response,
            SourceResponse::MusicUrl("https://cdn.example.test/track.flac".to_owned())
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
        assert!(!report.manifest.requires_rust_port);
        assert!(report
            .manifest
            .warnings
            .iter()
            .any(|warning| warning.contains("runtime initialization")));
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
    fn ast_analysis_should_preserve_utf8_string_arrays() {
        let parsed = parse_with_oxc(
            Path::new("unicode.js"),
            r#"const labels = ["网易云音乐", "晴天", "こんにちは"];
               globalThis.lx.send(globalThis.lx.EVENT_NAMES.inited, labels);"#,
        )
        .expect("Unicode JavaScript should parse");

        assert!(parsed.facts.string_arrays.iter().any(|values| {
            values
                == &[
                    "网易云音乐".to_owned(),
                    "晴天".to_owned(),
                    "こんにちは".to_owned(),
                ]
        }));
    }

    #[test]
    fn ast_analysis_should_extract_multiline_url_template_objects() {
        let report = analyze_lx_js_source(
            "templates.js",
            r#"
                const CUSTOM_URL_TEMPLATES = {
                    wy:
                        "https://example.test/resolve?id={id}&level={level}",
                    tx: `https://music.example.test/play?id={id}&level={level}`,
                };
                const { EVENT_NAMES, on, send } = globalThis.lx;
                on(EVENT_NAMES.request, () => "musicUrl");
                send(EVENT_NAMES.inited, { sources: { wy: "网易云音乐", tx: "QQ音乐" } });
                const qualitys = ["128k", "320k"];
            "#,
        )
        .expect("template JavaScript should analyze");

        assert_eq!(report.endpoint.templates.len(), 2);
        assert!(report.endpoint.templates.iter().all(|template| {
            template.family == "custom"
                && template.has_track_id_placeholder
                && template.has_quality_placeholder
        }));
    }

    #[test]
    fn dynamic_music_url_contract_should_select_quickjs_without_static_templates() {
        let report = analyze_lx_js_source(
            "dynamic.js",
            r#"
                const { EVENT_NAMES, on, send } = globalThis.lx;
                on(EVENT_NAMES.request, ({ action }) => {
                  if (action !== 'musicUrl') return Promise.reject(new Error('unsupported'));
                  return Promise.resolve(['https:', '', 'cdn.example.test', 'song.mp3'].join('/'));
                });
                send(EVENT_NAMES.inited, {
                  sources: { kg: { name: 'Kugou', type: 'music', actions: ['musicUrl'], qualitys: ['128k'] } },
                });
            "#,
        )
        .expect("dynamic LX JavaScript should analyze");

        assert!(report.endpoint.templates.is_empty());
        assert_eq!(
            supported_import_adapter(&report),
            Some(LxJsImportAdapter::QuickJs)
        );
    }

    #[test]
    fn legacy_named_adapter_should_use_only_templates_from_the_imported_script() {
        let report = analyze_lx_js_source(
            "fresh-nianxin.js",
            r#"
                const NIANXIN_URL_TEMPLATES = {
                  kg: 'https://fresh.example.test/kg.php?id={id}&level={level}',
                };
                const { EVENT_NAMES, on, send } = globalThis.lx;
                on(EVENT_NAMES.request, () => Promise.resolve('musicUrl'));
                send(EVENT_NAMES.inited, {
                  sources: { kg: { name: 'Kugou', type: 'music', actions: ['musicUrl'], qualitys: ['128k'] } },
                });
            "#,
        )
        .expect("legacy-named LX source should analyze");
        let templates = import_adapter_templates(&report, LxJsImportAdapter::Nianxin)
            .expect("script-derived Nianxin template should import");

        assert_eq!(templates.len(), 1);
        assert!(templates[0].url.contains("fresh.example.test"));
        assert!(templates
            .iter()
            .all(|template| !template.url.contains("music.nxinxz.com")));
    }

    #[test]
    fn metadata_should_be_read_from_all_parser_comments() {
        let source = format!(
            "{}\n/**\n * @name Late Metadata\n * @version 2.0.0\n */\nconst value = 'musicUrl';",
            "\n".repeat(150)
        );

        let report = analyze_lx_js_source("metadata.js", &source)
            .expect("metadata JavaScript should analyze");

        assert_eq!(report.metadata.name.as_deref(), Some("Late Metadata"));
        assert_eq!(report.metadata.version.as_deref(), Some("2.0.0"));
    }

    #[test]
    fn imported_provider_identity_should_remain_stable_across_source_versions() {
        let first = LxJsMetadata {
            name: Some("Versioned Source".to_owned()),
            version: Some("1.0.0".to_owned()),
            author: Some("Fika Tests".to_owned()),
            ..LxJsMetadata::default()
        };
        let mut second = first.clone();
        second.version = Some("2.0.0".to_owned());

        assert_eq!(
            provider_id_from_metadata(&first),
            provider_id_from_metadata(&second)
        );
    }

    #[test]
    fn append_query_params_should_preserve_existing_query_and_fragment() {
        let result = append_query_params(
            "https://example.test/search?existing=1#player",
            &[
                ("keyword", "晴天 & friends".to_owned()),
                ("mode", "lossless".to_owned()),
            ],
        )
        .expect("HTTP URL should accept query parameters");
        let url = url::Url::parse(&result).expect("result should remain a valid URL");
        let pairs = url.query_pairs().into_owned().collect::<BTreeMap<_, _>>();

        assert_eq!(url.fragment(), Some("player"));
        assert_eq!(pairs.get("existing").map(String::as_str), Some("1"));
        assert_eq!(
            pairs.get("keyword").map(String::as_str),
            Some("晴天 & friends")
        );
        assert_eq!(pairs.get("mode").map(String::as_str), Some("lossless"));
    }

    #[test]
    fn render_template_url_should_encode_placeholders_and_reject_non_http_schemes() {
        let rendered = render_template_url(
            "https://example.test/play/{id}?level={level}",
            "晴天 / live?",
            "lossless",
        )
        .expect("HTTP template should render");

        assert!(rendered.contains("%E6%99%B4%E5%A4%A9%20%2F%20live%3F"));
        assert!(render_template_url("file:///tmp/{id}", "track", "standard").is_err());
    }

    #[test]
    fn extract_domain_should_use_url_host_parsing() {
        assert_eq!(
            extract_domain("https://user:secret@[2001:db8::1]:8443/play?id=1"),
            Some("[2001:db8::1]".to_owned())
        );
    }

    #[test]
    fn invalid_javascript_should_fail_before_import_report_is_created() {
        let error = analyze_lx_js_source("broken.js", "const = ;")
            .expect_err("invalid JS should fail OXC parsing");

        assert!(matches!(error, LxJsImportError::Parse { .. }));
    }
}
