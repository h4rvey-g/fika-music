//! Constrained LX JavaScript execution behind the existing Source Runtime host interface.

use crate::lx_js_importer::LxJsMetadata;
use crate::source_runtime::{
    SourceCapability, SourceHttpMethod, SourceHttpRequest, SourceHttpResponse, SourceInfo,
    SourceProvider, SourceRequest, SourceResponse, SourceRuntimeContext, SourceRuntimeError,
};
use aes::Aes128;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyInit, KeyIvInit};
use rand::rngs::OsRng;
use rand::RngCore;
use rquickjs::promise::MaybePromise;
use rquickjs::{Context, Ctx, Function, Runtime, Value};
use rsa::pkcs1::DecodeRsaPublicKey;
use rsa::pkcs8::DecodePublicKey;
use rsa::{Pkcs1v15Encrypt, RsaPublicKey};
use serde::Deserialize;
use serde_json::{json, Value as JsonValue};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::time::{Duration, Instant};
use url::form_urlencoded;

const LX_JS_MEMORY_LIMIT: usize = 96 * 1024 * 1024;
const LX_JS_STACK_LIMIT: usize = 512 * 1024;
const LX_JS_EXECUTION_TIMEOUT: Duration = Duration::from_secs(30);
const LX_JS_MAX_PENDING_JOBS: usize = 20_000;
const LX_JS_MAX_HTTP_REQUESTS: usize = 16;
const LX_JS_MAX_REQUEST_BODY_BYTES: usize = 4 * 1024 * 1024;
const LX_JS_MAX_HEADERS: usize = 64;
const LX_JS_MAX_HEADER_BYTES: usize = 16 * 1024;
const LX_JS_MAX_LOG_BYTES: usize = 1_024;
const LX_JS_MAX_RANDOM_BYTES: usize = 4 * 1024;
const LX_JS_MAX_AES_INPUT_BYTES: usize = 1024 * 1024;
const LX_JS_MAX_RSA_INPUT_BYTES: usize = 16 * 1024;
const LX_JS_MAX_RSA_KEY_BYTES: usize = 64 * 1024;

const LX_JS_BOOTSTRAP: &str = r#"
(() => {
  'use strict';

  const nativeRequest = globalThis.__fikaNativeRequest;
  const nativeSend = globalThis.__fikaNativeSend;
  const nativeLog = globalThis.__fikaNativeLog;
  const nativeMd5 = globalThis.__fikaNativeMd5;
  const nativeRandom = globalThis.__fikaNativeRandom;
  const nativeAes = globalThis.__fikaNativeAes;
  const nativeRsa = globalThis.__fikaNativeRsa;
  const scriptInfo = JSON.parse(globalThis.__fikaScriptInfoJson);
  const handlers = Object.create(null);
  const timers = new Map();
  let nextTimerId = 1;
  let initialized = false;

  delete globalThis.__fikaNativeRequest;
  delete globalThis.__fikaNativeSend;
  delete globalThis.__fikaNativeLog;
  delete globalThis.__fikaNativeMd5;
  delete globalThis.__fikaNativeRandom;
  delete globalThis.__fikaNativeAes;
  delete globalThis.__fikaNativeRsa;
  delete globalThis.__fikaScriptInfoJson;

  const EVENT_NAMES = Object.freeze({
    inited: 'inited',
    request: 'request',
    updateAlert: 'updateAlert',
  });

  function errorMessage(error) {
    if (error && typeof error.message === 'string') return error.message;
    try { return String(error); } catch (_) { return 'Unknown LX script error'; }
  }

  function formatLogArg(value) {
    if (value instanceof Error) return value.stack || value.message;
    if (typeof value === 'string') return value;
    try {
      const encoded = JSON.stringify(value);
      return encoded === undefined ? String(value) : encoded;
    } catch (_) {
      try { return String(value); } catch (_) { return '<unprintable>'; }
    }
  }

  function utf8Encode(value) {
    const encoded = unescape(encodeURIComponent(String(value)));
    const output = new Uint8Array(encoded.length);
    for (let index = 0; index < encoded.length; index += 1) {
      output[index] = encoded.charCodeAt(index);
    }
    return output;
  }

  function utf8Decode(value) {
    let binary = '';
    for (const byte of asBytes(value)) binary += String.fromCharCode(byte);
    try { return decodeURIComponent(escape(binary)); } catch (_) { return binary; }
  }

  function asBytes(value) {
    if (value instanceof Uint8Array) return value;
    if (value instanceof ArrayBuffer) return new Uint8Array(value);
    if (ArrayBuffer.isView(value)) {
      return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
    }
    if (Array.isArray(value)) return Uint8Array.from(value);
    if (value && typeof value === 'object') {
      return Uint8Array.from(Object.keys(value).sort((a, b) => Number(a) - Number(b)).map(key => value[key]));
    }
    return utf8Encode(value == null ? '' : value);
  }

  function bytesToHex(value) {
    let output = '';
    for (const byte of asBytes(value)) output += byte.toString(16).padStart(2, '0');
    return output;
  }

  function hexToBytes(value) {
    const text = String(value).replace(/\s+/g, '');
    if (text.length % 2 !== 0 || !/^[0-9a-f]*$/i.test(text)) throw new Error('Invalid hex input');
    const output = new Uint8Array(text.length / 2);
    for (let index = 0; index < text.length; index += 2) {
      output[index / 2] = parseInt(text.slice(index, index + 2), 16);
    }
    return output;
  }

  const BASE64_ALPHABET = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';

  function bytesToBase64(value) {
    const bytes = asBytes(value);
    let output = '';
    for (let index = 0; index < bytes.length; index += 3) {
      const first = bytes[index];
      const second = index + 1 < bytes.length ? bytes[index + 1] : 0;
      const third = index + 2 < bytes.length ? bytes[index + 2] : 0;
      const block = (first << 16) | (second << 8) | third;
      output += BASE64_ALPHABET[(block >> 18) & 63];
      output += BASE64_ALPHABET[(block >> 12) & 63];
      output += index + 1 < bytes.length ? BASE64_ALPHABET[(block >> 6) & 63] : '=';
      output += index + 2 < bytes.length ? BASE64_ALPHABET[block & 63] : '=';
    }
    return output;
  }

  function base64ToBytes(value) {
    const text = String(value).replace(/\s+/g, '').replace(/-/g, '+').replace(/_/g, '/');
    if (text.length % 4 === 1 || !/^[A-Za-z0-9+/]*={0,2}$/.test(text)) {
      throw new Error('Invalid base64 input');
    }
    const padded = text.padEnd(Math.ceil(text.length / 4) * 4, '=');
    const output = [];
    for (let index = 0; index < padded.length; index += 4) {
      const a = BASE64_ALPHABET.indexOf(padded[index]);
      const b = BASE64_ALPHABET.indexOf(padded[index + 1]);
      const c = padded[index + 2] === '=' ? 0 : BASE64_ALPHABET.indexOf(padded[index + 2]);
      const d = padded[index + 3] === '=' ? 0 : BASE64_ALPHABET.indexOf(padded[index + 3]);
      const block = (a << 18) | (b << 12) | (c << 6) | d;
      output.push((block >> 16) & 255);
      if (padded[index + 2] !== '=') output.push((block >> 8) & 255);
      if (padded[index + 3] !== '=') output.push(block & 255);
    }
    return Uint8Array.from(output);
  }

  const lxBuffers = new WeakSet();

  function makeBuffer(value) {
    const output = new Uint8Array(asBytes(value));
    lxBuffers.add(output);
    Object.defineProperty(output, 'toString', {
      value(format = 'utf8') { return buffer.bufToString(output, format); },
      writable: false,
      configurable: false,
    });
    return output;
  }

  const buffer = Object.freeze({
    from(value, format = 'utf8') {
      if (typeof value !== 'string') return makeBuffer(value);
      const normalized = String(format).toLowerCase().replace('-', '');
      if (normalized === 'base64') return makeBuffer(base64ToBytes(value));
      if (normalized === 'hex') return makeBuffer(hexToBytes(value));
      if (normalized === 'utf8' || normalized === 'utf') return makeBuffer(utf8Encode(value));
      throw new Error(`Unsupported buffer format: ${format}`);
    },
    bufToString(value, format = 'utf8') {
      const normalized = String(format).toLowerCase().replace('-', '');
      if (normalized === 'base64') return bytesToBase64(value);
      if (normalized === 'hex') return bytesToHex(value);
      if (normalized === 'utf8' || normalized === 'utf') return utf8Decode(value);
      throw new Error(`Unsupported buffer format: ${format}`);
    },
  });

  function cryptoResult(encoded) {
    const result = JSON.parse(encoded);
    if (!result.ok) throw new Error(result.error || 'LX crypto operation failed');
    return makeBuffer(base64ToBytes(result.data));
  }

  const crypto = Object.freeze({
    md5(value) { return nativeMd5(String(value)); },
    randomBytes(size) {
      const amount = Number(size);
      if (!Number.isSafeInteger(amount) || amount < 0) throw new Error('Invalid random byte count');
      return makeBuffer(base64ToBytes(nativeRandom(amount)));
    },
    aesEncrypt(value, mode, key, iv) {
      if (asBytes(value).byteLength > 1048576) throw new Error('AES input exceeds the configured size limit');
      return cryptoResult(nativeAes(
        bytesToBase64(value),
        String(mode),
        bytesToBase64(key),
        iv == null ? '' : bytesToBase64(iv),
      ));
    },
    rsaEncrypt(value, key) {
      if (asBytes(value).byteLength > 16384) throw new Error('RSA input exceeds the configured size limit');
      if (String(key).length > 65536) throw new Error('RSA key exceeds the configured size limit');
      return cryptoResult(nativeRsa(bytesToBase64(value), String(key)));
    },
  });

  function request(url, options, callback) {
    if (typeof callback !== 'function') throw new TypeError('LX request callback is required');
    let cancelled = false;
    let result;
    try {
      result = JSON.parse(nativeRequest(String(url), JSON.stringify(options || {})));
    } catch (error) {
      callback(new Error(errorMessage(error)), null, null);
      return () => { cancelled = true; };
    }
    if (!cancelled) {
      if (result.ok) callback(null, result.response, result.response.body);
      else callback(new Error(result.error || 'LX network request failed'), null, null);
    }
    return () => { cancelled = true; };
  }

  function on(eventName, handler) {
    if (typeof eventName !== 'string' || typeof handler !== 'function') {
      throw new TypeError('LX event registration requires a name and handler');
    }
    handlers[eventName] = handler;
  }

  function send(eventName, data) {
    if (eventName === EVENT_NAMES.inited) {
      const sources = data && typeof data === 'object' && !Array.isArray(data) ? data.sources : null;
      if (!sources || typeof sources !== 'object' || Array.isArray(sources) || Object.keys(sources).length === 0) {
        throw new TypeError('LX inited event requires a non-empty sources object');
      }
      initialized = true;
    }
    let encoded = 'null';
    try { encoded = JSON.stringify(data === undefined ? null : data).slice(0, 16384); } catch (_) {}
    nativeSend(String(eventName), encoded);
  }

  function scheduleTimer(handler, _delay, ...args) {
    if (typeof handler !== 'function') throw new TypeError('Timer callback must be a function');
    const id = nextTimerId++;
    timers.set(id, true);
    // QuickJS has no ambient event loop, so timers are cancellable microtasks.
    Promise.resolve().then(() => {
      if (!timers.delete(id)) return;
      handler(...args);
    });
    return id;
  }

  function clearTimer(id) {
    timers.delete(Number(id));
  }

  const lx = Object.freeze({
    version: '1.2.0',
    env: 'desktop',
    currentScriptInfo: Object.freeze(scriptInfo),
    EVENT_NAMES,
    on,
    send,
    request,
    utils: Object.freeze({ buffer, crypto }),
  });

  Object.defineProperty(globalThis, 'lx', {
    value: lx,
    writable: false,
    configurable: false,
    enumerable: true,
  });
  Object.defineProperty(globalThis, 'Buffer', {
    value: Object.freeze({
      from: buffer.from,
      isBuffer(value) { return lxBuffers.has(value); },
    }),
    writable: false,
    configurable: false,
  });
  Object.defineProperty(globalThis, 'console', {
    value: Object.freeze({
      log: (...args) => nativeLog('info', args.map(formatLogArg).join(' ').slice(0, 1024)),
      info: (...args) => nativeLog('info', args.map(formatLogArg).join(' ').slice(0, 1024)),
      warn: (...args) => nativeLog('warn', args.map(formatLogArg).join(' ').slice(0, 1024)),
      error: (...args) => nativeLog('error', args.map(formatLogArg).join(' ').slice(0, 1024)),
      debug: (...args) => nativeLog('debug', args.map(formatLogArg).join(' ').slice(0, 1024)),
    }),
    writable: false,
    configurable: false,
  });
  Object.defineProperty(globalThis, 'setTimeout', {
    value: scheduleTimer,
    writable: false,
    configurable: false,
  });
  Object.defineProperty(globalThis, 'clearTimeout', {
    value: clearTimer,
    writable: false,
    configurable: false,
  });

  Object.defineProperty(globalThis, '__fikaLxReady', {
    value: () => initialized && typeof handlers[EVENT_NAMES.request] === 'function',
    writable: false,
    configurable: false,
  });
  Object.defineProperty(globalThis, '__fikaLxInvoke', {
    value: payloadJson => Promise.resolve().then(() => {
      if (!initialized) throw new Error('LX source did not send the inited event');
      const handler = handlers[EVENT_NAMES.request];
      if (typeof handler !== 'function') throw new Error('LX source did not register a request handler');
      return handler(JSON.parse(payloadJson));
    }).then(
      value => JSON.stringify({ ok: true, value }),
      error => JSON.stringify({ ok: false, error: errorMessage(error) }),
    ),
    writable: false,
    configurable: false,
  });
})();
"#;

#[derive(Debug, Clone, Copy)]
struct LxJsLimits {
    memory_bytes: usize,
    stack_bytes: usize,
    execution_timeout: Duration,
    max_pending_jobs: usize,
    max_http_requests: usize,
}

impl Default for LxJsLimits {
    fn default() -> Self {
        Self {
            memory_bytes: LX_JS_MEMORY_LIMIT,
            stack_bytes: LX_JS_STACK_LIMIT,
            execution_timeout: LX_JS_EXECUTION_TIMEOUT,
            max_pending_jobs: LX_JS_MAX_PENDING_JOBS,
            max_http_requests: LX_JS_MAX_HTTP_REQUESTS,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ImportedLxJsProvider {
    provider_id: String,
    display_name: String,
    source: String,
    metadata: LxJsMetadata,
    source_catalog: BTreeMap<String, SourceInfo>,
    limits: LxJsLimits,
}

impl ImportedLxJsProvider {
    pub fn new(
        provider_id: impl Into<String>,
        display_name: impl Into<String>,
        source: impl Into<String>,
        metadata: LxJsMetadata,
        source_catalog: BTreeMap<String, SourceInfo>,
    ) -> Self {
        Self {
            provider_id: provider_id.into(),
            display_name: display_name.into(),
            source: source.into(),
            metadata,
            source_catalog,
            limits: LxJsLimits::default(),
        }
    }

    fn execute(
        &self,
        context: &mut SourceRuntimeContext,
        payload: Option<&JsonValue>,
    ) -> Result<Option<JsonValue>, SourceRuntimeError> {
        let deadline = Instant::now() + self.limits.execution_timeout;
        let cancellation = context.cancellation_token();
        let runtime = Runtime::new().map_err(|error| {
            context.provider_error(format!("could not create LX JavaScript runtime: {error}"))
        })?;
        runtime.set_memory_limit(self.limits.memory_bytes);
        runtime.set_max_stack_size(self.limits.stack_bytes);
        let interrupt_cancellation = cancellation.clone();
        runtime.set_interrupt_handler(Some(Box::new(move || {
            interrupt_cancellation.is_cancelled() || Instant::now() >= deadline
        })));
        let js_context = Context::full(&runtime).map_err(|error| {
            context.provider_error(format!("could not create LX JavaScript context: {error}"))
        })?;

        let script_info_json = self.script_info_json();
        let host_context = Rc::new(RefCell::new(context.fork_for_host_calls()));
        let request_count = Rc::new(Cell::new(0_usize));
        let result = js_context.with(|ctx| {
            install_native_bindings(
                &ctx,
                Rc::clone(&host_context),
                Rc::clone(&request_count),
                self.limits,
                deadline,
                script_info_json,
            )?;
            ctx.eval::<(), _>(LX_JS_BOOTSTRAP)
                .map_err(|error| js_failure(&ctx, "install LX host", error))?;
            let source_error = match ctx.eval::<Value<'_>, _>(self.source.as_bytes()) {
                Ok(_) => None,
                Err(error) => {
                    let is_script_exception = error.is_exception();
                    let error = js_failure(&ctx, "evaluate source.js", error);
                    if !is_script_exception {
                        return Err(error);
                    }
                    Some(error)
                }
            };
            if let Some(error) = source_error {
                if !is_lx_source_ready(&ctx)? {
                    return Err(error);
                }
                host_context.borrow_mut().warn(format!(
                    "LX JavaScript reported an error after initialization: {}",
                    error
                ));
            }
            drain_jobs(&ctx, self.limits, deadline)?;

            if !is_lx_source_ready(&ctx)? {
                return Err(LxJsFailure::Script(
                    "source.js did not complete the LX inited/request contract".to_owned(),
                ));
            }

            let Some(payload) = payload else {
                return Ok(None);
            };
            let payload_json = serde_json::to_string(payload).map_err(|error| {
                LxJsFailure::Host(format!("could not encode LX request payload: {error}"))
            })?;
            let invoke: Function<'_> = ctx
                .globals()
                .get("__fikaLxInvoke")
                .map_err(|error| js_failure(&ctx, "read LX request handler", error))?;
            let promise = invoke
                .call::<_, MaybePromise<'_>>((payload_json,))
                .map_err(|error| js_failure(&ctx, "invoke LX request handler", error))?;
            let encoded = finish_promise(&ctx, &promise, self.limits, deadline)?;
            decode_invocation_result(&encoded)
        });
        let nested_diagnostics = host_context.borrow().diagnostics().to_vec();
        context.append_nested_diagnostics(&nested_diagnostics);

        if cancellation.is_cancelled() {
            return context
                .ensure_not_cancelled("execute imported LX JavaScript")
                .map(|()| None);
        }
        if Instant::now() >= deadline {
            return Err(context.provider_error(format!(
                "LX JavaScript exceeded the {} second execution limit",
                self.limits.execution_timeout.as_secs()
            )));
        }
        result.map_err(|error| context.provider_error(error.to_string()))
    }

    fn script_info_json(&self) -> String {
        json!({
            "name": self.metadata.name,
            "description": self.metadata.description,
            "version": self.metadata.version,
            "author": self.metadata.author,
            "homepage": self.metadata.homepage,
            "rawScript": self.source,
        })
        .to_string()
    }

    #[cfg(test)]
    fn with_execution_timeout(mut self, timeout: Duration) -> Self {
        self.limits.execution_timeout = timeout;
        self
    }
}

impl SourceProvider for ImportedLxJsProvider {
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
        context.require_capability(
            SourceCapability::NetworkAny,
            "initialize imported LX JavaScript",
        )?;
        self.execute(context, None)?;
        context.info(format!(
            "initialized {} in the constrained QuickJS audio-source runtime",
            self.display_name
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
        context.require_capability(SourceCapability::NetworkAny, "execute imported LX musicUrl")?;
        let payload = json!({
            "source": source,
            "action": "musicUrl",
            "info": {
                "type": quality.as_str(),
                "musicInfo": music_info,
            },
        });
        let value = self
            .execute(context, Some(&payload))?
            .ok_or_else(|| context.provider_error("LX musicUrl handler did not return a result"))?;
        let url = value
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| context.provider_error("LX musicUrl handler did not return a URL"))?;
        if !is_http_url(url) {
            return Err(context.provider_error("LX musicUrl handler returned an invalid URL"));
        }
        let url = resolve_webview_media_url(context, url);
        context.info("resolved musicUrl through imported LX JavaScript");
        Ok(SourceResponse::MusicUrl(url))
    }
}

#[derive(Debug, thiserror::Error)]
enum LxJsFailure {
    #[error("LX JavaScript host failed: {0}")]
    Host(String),
    #[error("LX JavaScript failed: {0}")]
    Script(String),
    #[error("LX JavaScript execution limit exceeded")]
    Limit,
    #[error("LX JavaScript promise could not make progress")]
    WouldBlock,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LxInvocationResult {
    ok: bool,
    #[serde(default)]
    value: Option<JsonValue>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct LxHttpOptions {
    method: Option<String>,
    headers: BTreeMap<String, JsonValue>,
    body: Option<JsonValue>,
    form: Option<JsonValue>,
    form_data: Option<JsonValue>,
    timeout: Option<JsonValue>,
}

fn install_native_bindings<'js>(
    ctx: &Ctx<'js>,
    host_context: Rc<RefCell<SourceRuntimeContext>>,
    request_count: Rc<Cell<usize>>,
    limits: LxJsLimits,
    deadline: Instant,
    script_info_json: String,
) -> Result<(), LxJsFailure> {
    let globals = ctx.globals();
    let request_host_context = Rc::clone(&host_context);
    let request = Function::new(
        ctx.clone(),
        move |url: String, options_json: String| -> String {
            if Instant::now() >= deadline {
                return native_error("LX JavaScript execution limit exceeded");
            }
            let current_count = request_count.get();
            if current_count >= limits.max_http_requests {
                request_host_context
                    .borrow_mut()
                    .error("LX JavaScript exceeded its HTTP request limit");
                return native_error("LX JavaScript HTTP request limit exceeded");
            }
            request_count.set(current_count + 1);
            let options = match serde_json::from_str::<LxHttpOptions>(&options_json) {
                Ok(options) => options,
                Err(_) => return native_error("LX request options are invalid"),
            };
            let request = match build_http_request(url, options, deadline) {
                Ok(request) => request,
                Err(message) => return native_error(&message),
            };
            let response = request_host_context
                .borrow_mut()
                .http_request(request, "execute imported LX HTTP request");
            match response {
                Ok(response) => json!({
                    "ok": true,
                    "response": {
                        "statusCode": response.status,
                        "headers": response.headers,
                        "body": String::from_utf8_lossy(&response.body),
                    },
                })
                .to_string(),
                Err(error) => native_error(&error.to_string()),
            }
        },
    )
    .map_err(|error| LxJsFailure::Host(error.to_string()))?;
    globals
        .set("__fikaNativeRequest", request)
        .map_err(|error| LxJsFailure::Host(error.to_string()))?;

    let send_host_context = Rc::clone(&host_context);
    let send = Function::new(ctx.clone(), move |event_name: String, data_json: String| {
        let mut context = send_host_context.borrow_mut();
        match event_name.as_str() {
            "inited" => context.info("LX JavaScript sent the inited event"),
            "updateAlert" => {
                let message = serde_json::from_str::<JsonValue>(&data_json)
                    .ok()
                    .and_then(|value| {
                        value
                            .get("log")
                            .and_then(JsonValue::as_str)
                            .map(str::to_owned)
                    })
                    .unwrap_or_else(|| "source update is available".to_owned());
                context.warn(format!(
                    "LX source update notice: {}",
                    sanitize_script_message(&message)
                ));
            }
            _ => context.warn("LX JavaScript sent an unsupported event"),
        }
    })
    .map_err(|error| LxJsFailure::Host(error.to_string()))?;
    globals
        .set("__fikaNativeSend", send)
        .map_err(|error| LxJsFailure::Host(error.to_string()))?;

    let log_host_context = Rc::clone(&host_context);
    let log = Function::new(ctx.clone(), move |level: String, message: String| {
        let message = sanitize_script_message(&message);
        let mut context = log_host_context.borrow_mut();
        match level.as_str() {
            "warn" => context.warn(format!("LX script: {message}")),
            "error" => context.error(format!("LX script: {message}")),
            "debug" => {}
            _ => context.info(format!("LX script: {message}")),
        }
    })
    .map_err(|error| LxJsFailure::Host(error.to_string()))?;
    globals
        .set("__fikaNativeLog", log)
        .map_err(|error| LxJsFailure::Host(error.to_string()))?;

    let md5 = Function::new(ctx.clone(), |value: String| {
        format!("{:x}", md5::compute(value.as_bytes()))
    })
    .map_err(|error| LxJsFailure::Host(error.to_string()))?;
    globals
        .set("__fikaNativeMd5", md5)
        .map_err(|error| LxJsFailure::Host(error.to_string()))?;

    let random = Function::new(ctx.clone(), |size: i32| {
        let size = usize::try_from(size.max(0))
            .unwrap_or_default()
            .min(LX_JS_MAX_RANDOM_BYTES);
        let mut bytes = vec![0_u8; size];
        OsRng.fill_bytes(&mut bytes);
        BASE64.encode(bytes)
    })
    .map_err(|error| LxJsFailure::Host(error.to_string()))?;
    globals
        .set("__fikaNativeRandom", random)
        .map_err(|error| LxJsFailure::Host(error.to_string()))?;

    let aes = Function::new(
        ctx.clone(),
        |data: String, mode: String, key: String, iv: String| {
            crypto_envelope(aes_encrypt(&data, &mode, &key, &iv))
        },
    )
    .map_err(|error| LxJsFailure::Host(error.to_string()))?;
    globals
        .set("__fikaNativeAes", aes)
        .map_err(|error| LxJsFailure::Host(error.to_string()))?;

    let rsa = Function::new(ctx.clone(), |data: String, key: String| {
        crypto_envelope(rsa_encrypt(&data, &key))
    })
    .map_err(|error| LxJsFailure::Host(error.to_string()))?;
    globals
        .set("__fikaNativeRsa", rsa)
        .map_err(|error| LxJsFailure::Host(error.to_string()))?;
    globals
        .set("__fikaScriptInfoJson", script_info_json)
        .map_err(|error| LxJsFailure::Host(error.to_string()))?;
    Ok(())
}

pub(crate) fn build_http_request(
    url: String,
    options: LxHttpOptions,
    deadline: Instant,
) -> Result<SourceHttpRequest, String> {
    let method = match options
        .method
        .as_deref()
        .unwrap_or("GET")
        .to_ascii_uppercase()
        .as_str()
    {
        "GET" => SourceHttpMethod::Get,
        "POST" => SourceHttpMethod::Post,
        "PUT" => SourceHttpMethod::Put,
        "PATCH" => SourceHttpMethod::Patch,
        "DELETE" => SourceHttpMethod::Delete,
        "HEAD" => SourceHttpMethod::Head,
        _ => return Err("LX request uses an unsupported HTTP method".to_owned()),
    };
    let mut headers = BTreeMap::new();
    if options.headers.len() > LX_JS_MAX_HEADERS {
        return Err("LX request contains too many headers".to_owned());
    }
    for (name, value) in options.headers {
        if forbidden_header(&name) {
            return Err(format!("LX request may not set the {name} header"));
        }
        let value = json_scalar_text(&value)
            .ok_or_else(|| "LX request header values must be scalar".to_owned())?;
        if name.len().saturating_add(value.len()) > LX_JS_MAX_HEADER_BYTES {
            return Err("LX request header exceeds the configured size limit".to_owned());
        }
        headers.insert(name, value);
    }

    let mut body = match (options.form, options.form_data, options.body) {
        (Some(form), _, _) | (None, Some(form), _) => {
            headers
                .entry("content-type".to_owned())
                .or_insert_with(|| "application/x-www-form-urlencoded".to_owned());
            Some(encode_form(&form)?.into_bytes())
        }
        (None, None, Some(JsonValue::String(body))) => Some(body.into_bytes()),
        (None, None, Some(body)) => {
            headers
                .entry("content-type".to_owned())
                .or_insert_with(|| "application/json".to_owned());
            Some(
                serde_json::to_vec(&body)
                    .map_err(|_| "LX request body could not be encoded".to_owned())?,
            )
        }
        (None, None, None) => None,
    };
    if body
        .as_ref()
        .is_some_and(|body| body.len() > LX_JS_MAX_REQUEST_BODY_BYTES)
    {
        return Err("LX request body exceeds the configured size limit".to_owned());
    }

    let requested_timeout = options
        .timeout
        .as_ref()
        .and_then(json_u64)
        .filter(|value| *value > 0);
    let remaining = deadline.saturating_duration_since(Instant::now());
    let timeout = requested_timeout
        .map(Duration::from_millis)
        .unwrap_or(remaining)
        .min(remaining)
        .max(Duration::from_millis(1));
    Ok(SourceHttpRequest {
        method,
        url,
        headers,
        body: body.take(),
        json_body: None,
        timeout: Some(timeout),
    })
}

fn forbidden_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "host"
            | "content-length"
            | "transfer-encoding"
            | "connection"
            | "proxy-connection"
            | "upgrade"
    )
}

fn encode_form(value: &JsonValue) -> Result<String, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "LX request form must be an object".to_owned())?;
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    for (key, value) in object {
        let value = json_scalar_text(value)
            .unwrap_or_else(|| serde_json::to_string(value).unwrap_or_default());
        serializer.append_pair(key, &value);
    }
    Ok(serializer.finish())
}

fn json_scalar_text(value: &JsonValue) -> Option<String> {
    match value {
        JsonValue::String(value) => Some(value.clone()),
        JsonValue::Number(value) => Some(value.to_string()),
        JsonValue::Bool(value) => Some(value.to_string()),
        JsonValue::Null => Some(String::new()),
        JsonValue::Array(_) | JsonValue::Object(_) => None,
    }
}

fn json_u64(value: &JsonValue) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
}

fn drain_jobs(ctx: &Ctx<'_>, limits: LxJsLimits, deadline: Instant) -> Result<(), LxJsFailure> {
    for _ in 0..limits.max_pending_jobs {
        if Instant::now() >= deadline {
            return Err(LxJsFailure::Limit);
        }
        if !ctx.execute_pending_job() {
            return Ok(());
        }
        if ctx.has_exception() {
            return Err(LxJsFailure::Script(caught_exception(ctx)));
        }
    }
    Err(LxJsFailure::Limit)
}

fn is_lx_source_ready(ctx: &Ctx<'_>) -> Result<bool, LxJsFailure> {
    let ready: Function<'_> = ctx
        .globals()
        .get("__fikaLxReady")
        .map_err(|error| js_failure(ctx, "read LX initialization state", error))?;
    ready
        .call::<_, bool>(())
        .map_err(|error| js_failure(ctx, "read LX initialization state", error))
}

fn finish_promise(
    ctx: &Ctx<'_>,
    promise: &MaybePromise<'_>,
    limits: LxJsLimits,
    deadline: Instant,
) -> Result<String, LxJsFailure> {
    for _ in 0..limits.max_pending_jobs {
        if let Some(result) = promise.result::<String>() {
            return result.map_err(|error| js_failure(ctx, "resolve LX request", error));
        }
        if Instant::now() >= deadline {
            return Err(LxJsFailure::Limit);
        }
        if !ctx.execute_pending_job() {
            return Err(LxJsFailure::WouldBlock);
        }
    }
    Err(LxJsFailure::Limit)
}

fn decode_invocation_result(encoded: &str) -> Result<Option<JsonValue>, LxJsFailure> {
    let result = serde_json::from_str::<LxInvocationResult>(encoded)
        .map_err(|error| LxJsFailure::Script(format!("invalid handler result: {error}")))?;
    if result.ok {
        Ok(result.value)
    } else {
        Err(LxJsFailure::Script(sanitize_script_message(
            &result
                .error
                .unwrap_or_else(|| "musicUrl handler rejected".to_owned()),
        )))
    }
}

fn js_failure(ctx: &Ctx<'_>, operation: &str, error: rquickjs::Error) -> LxJsFailure {
    if error.is_exception() {
        LxJsFailure::Script(format!("{operation}: {}", caught_exception(ctx)))
    } else {
        LxJsFailure::Script(format!("{operation}: {error}"))
    }
}

fn caught_exception(ctx: &Ctx<'_>) -> String {
    let value = ctx.catch();
    let message = value
        .as_exception()
        .and_then(|error| error.message())
        .unwrap_or_else(|| format!("uncaught {}", value.type_name()));
    sanitize_script_message(&message)
}

fn native_error(message: &str) -> String {
    json!({
        "ok": false,
        "error": sanitize_script_message(message),
    })
    .to_string()
}

fn crypto_envelope(result: Result<Vec<u8>, String>) -> String {
    match result {
        Ok(bytes) => json!({ "ok": true, "data": BASE64.encode(bytes) }).to_string(),
        Err(error) => native_error(&error),
    }
}

fn aes_encrypt(data: &str, mode: &str, key: &str, iv: &str) -> Result<Vec<u8>, String> {
    let data = decode_base64_bounded(data, LX_JS_MAX_AES_INPUT_BYTES, "AES input")?;
    let key = BASE64
        .decode(key)
        .map_err(|_| "AES key is not valid base64".to_owned())?;
    if key.len() != 16 {
        return Err("AES-128 requires a 16-byte key".to_owned());
    }
    match mode.to_ascii_lowercase().as_str() {
        "aes-128-cbc" => {
            let iv = BASE64
                .decode(iv)
                .map_err(|_| "AES IV is not valid base64".to_owned())?;
            if iv.len() != 16 {
                return Err("AES-128-CBC requires a 16-byte IV".to_owned());
            }
            let encryptor = cbc::Encryptor::<Aes128>::new_from_slices(&key, &iv)
                .map_err(|_| "AES-128-CBC parameters are invalid".to_owned())?;
            Ok(encryptor.encrypt_padded_vec_mut::<Pkcs7>(&data))
        }
        "aes-128-ecb" => {
            let encryptor = ecb::Encryptor::<Aes128>::new_from_slice(&key)
                .map_err(|_| "AES-128-ECB parameters are invalid".to_owned())?;
            Ok(encryptor.encrypt_padded_vec_mut::<Pkcs7>(&data))
        }
        _ => Err("unsupported LX AES mode".to_owned()),
    }
}

fn rsa_encrypt(data: &str, key: &str) -> Result<Vec<u8>, String> {
    let data = decode_base64_bounded(data, LX_JS_MAX_RSA_INPUT_BYTES, "RSA input")?;
    if key.len() > LX_JS_MAX_RSA_KEY_BYTES {
        return Err("RSA key exceeds the configured size limit".to_owned());
    }
    let public_key = RsaPublicKey::from_public_key_pem(key)
        .or_else(|_| RsaPublicKey::from_pkcs1_pem(key))
        .map_err(|_| "RSA public key is invalid".to_owned())?;
    public_key
        .encrypt(&mut OsRng, Pkcs1v15Encrypt, &data)
        .map_err(|_| "RSA encryption failed".to_owned())
}

fn decode_base64_bounded(value: &str, max_bytes: usize, label: &str) -> Result<Vec<u8>, String> {
    let max_encoded_bytes = max_bytes.saturating_mul(4).div_ceil(3).saturating_add(4);
    if value.len() > max_encoded_bytes {
        return Err(format!("{label} exceeds the configured size limit"));
    }
    let decoded = BASE64
        .decode(value)
        .map_err(|_| format!("{label} is not valid base64"))?;
    if decoded.len() > max_bytes {
        return Err(format!("{label} exceeds the configured size limit"));
    }
    Ok(decoded)
}

fn truncate_text(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

fn sanitize_script_message(value: &str) -> String {
    let mut output = String::with_capacity(value.len().min(LX_JS_MAX_LOG_BYTES));
    for (index, token) in value.split_whitespace().enumerate() {
        if index > 0 {
            output.push(' ');
        }
        let scheme_offset = token.find("https://").or_else(|| token.find("http://"));
        let query_offset = scheme_offset.and_then(|scheme_offset| {
            token[scheme_offset..]
                .find('?')
                .map(|query_offset| scheme_offset + query_offset)
        });
        if let Some(query_offset) = query_offset {
            output.push_str(&token[..query_offset]);
            output.push_str("?<redacted>");
        } else {
            output.push_str(token);
        }
        if output.len() >= LX_JS_MAX_LOG_BYTES {
            break;
        }
    }
    truncate_text(&output, LX_JS_MAX_LOG_BYTES)
}

fn is_http_url(value: &str) -> bool {
    url::Url::parse(value)
        .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
}

pub(crate) fn resolve_webview_media_url(context: &mut SourceRuntimeContext, value: &str) -> String {
    let Ok(mut candidate) = url::Url::parse(value) else {
        return value.to_owned();
    };
    if candidate.scheme() == "https" && has_media_extension(&candidate) {
        return value.to_owned();
    }
    if candidate.scheme() == "http" && candidate.set_scheme("https").is_err() {
        return value.to_owned();
    }

    let Some(response) = probe_media_redirect(context, candidate.as_str()) else {
        return value.to_owned();
    };
    let Ok(mut final_url) = url::Url::parse(&response.final_url) else {
        return value.to_owned();
    };
    if response.is_success() && final_url.scheme() == "https" {
        return final_url.to_string();
    }
    if final_url.scheme() != "http" || final_url.set_scheme("https").is_err() {
        return value.to_owned();
    }

    let Some(secure_response) = probe_media_redirect(context, final_url.as_str()) else {
        context.warn("imported LX musicUrl resolves to HTTP media that could not be upgraded");
        return value.to_owned();
    };
    let Ok(secure_final_url) = url::Url::parse(&secure_response.final_url) else {
        return value.to_owned();
    };
    if secure_response.is_success() && secure_final_url.scheme() == "https" {
        context.info("upgraded imported LX media redirect to HTTPS for WebView playback");
        return secure_final_url.to_string();
    }

    context.warn("imported LX musicUrl resolves to HTTP media that could not be upgraded");
    value.to_owned()
}

fn probe_media_redirect(
    context: &mut SourceRuntimeContext,
    value: &str,
) -> Option<SourceHttpResponse> {
    context
        .http_request(
            SourceHttpRequest {
                method: SourceHttpMethod::Head,
                url: value.to_owned(),
                headers: BTreeMap::new(),
                body: None,
                json_body: None,
                timeout: Some(Duration::from_secs(8)),
            },
            "resolve imported LX media redirect",
        )
        .ok()
}

fn has_media_extension(value: &url::Url) -> bool {
    let path = value.path().to_ascii_lowercase();
    ["mp3", "flac", "m4a", "mp4", "aac", "ogg", "opus", "wav"]
        .iter()
        .any(|extension| path.ends_with(&format!(".{extension}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_runtime::{
        lx_music_source, SourceAction, SourceCancellationToken, SourceHost, SourceHostError,
        SourceHttpResponse, SourceQuality, SourceRequestOutcome, SourceRuntime, LX_SOURCE_KG,
    };
    use std::io::Read;
    use std::sync::{Arc, Mutex};

    const TEST_SOURCE: &str = r#"
        const { EVENT_NAMES, on, request, send } = globalThis.lx;
        on(EVENT_NAMES.request, ({ source, action, info }) => new Promise((resolve, reject) => {
          request('https://resolver.example.test/play', {
            method: 'POST',
            headers: { 'x-source': source },
            body: { id: info.musicInfo.id, quality: info.type },
            timeout: 2500,
          }, (error, response) => {
            if (error) return reject(error);
            resolve(JSON.parse(response.body).url);
          });
        }));
        send(EVENT_NAMES.inited, {
          sources: { kg: { name: 'Kugou', type: 'music', actions: ['musicUrl'], qualitys: ['128k'] } },
        });
    "#;

    #[derive(Debug, Default)]
    struct RecordingHost {
        requests: Mutex<Vec<SourceHttpRequest>>,
    }

    #[derive(Debug, Default)]
    struct RedirectingMediaHost {
        requests: Mutex<Vec<SourceHttpRequest>>,
    }

    impl SourceHost for RecordingHost {
        fn http_request(
            &self,
            _source_id: &str,
            request: &SourceHttpRequest,
            cancellation: &SourceCancellationToken,
        ) -> Result<SourceHttpResponse, SourceHostError> {
            if cancellation.is_cancelled() {
                return Err(SourceHostError::Cancelled);
            }
            self.requests
                .lock()
                .expect("request log should lock")
                .push(request.clone());
            Ok(SourceHttpResponse {
                status: 200,
                final_url: request.url.clone(),
                headers: BTreeMap::from([(
                    "content-type".to_owned(),
                    "application/json".to_owned(),
                )]),
                content_type: Some("application/json".to_owned()),
                body: br#"{"url":"https://cdn.example.test/song.mp3"}"#.to_vec(),
            })
        }
    }

    impl SourceHost for RedirectingMediaHost {
        fn http_request(
            &self,
            _source_id: &str,
            request: &SourceHttpRequest,
            cancellation: &SourceCancellationToken,
        ) -> Result<SourceHttpResponse, SourceHostError> {
            if cancellation.is_cancelled() {
                return Err(SourceHostError::Cancelled);
            }
            self.requests
                .lock()
                .expect("request log should lock")
                .push(request.clone());
            let final_url = match request.url.as_str() {
                "https://resolver.example.test/play" => "http://media.example.test/song.flac",
                "https://media.example.test/song.flac" => "https://media.example.test/song.flac",
                _ => {
                    return Err(SourceHostError::Network {
                        url: request.url.clone(),
                        message: "unexpected test URL".to_owned(),
                    });
                }
            };
            Ok(SourceHttpResponse {
                status: 200,
                final_url: final_url.to_owned(),
                headers: BTreeMap::from([("content-type".to_owned(), "audio/flac".to_owned())]),
                content_type: Some("audio/flac".to_owned()),
                body: Vec::new(),
            })
        }
    }

    fn provider(source: &str) -> ImportedLxJsProvider {
        ImportedLxJsProvider::new(
            "test-lx-provider",
            "Test LX Source",
            source,
            LxJsMetadata {
                name: Some("Test LX Source".to_owned()),
                version: Some("1.0.0".to_owned()),
                ..LxJsMetadata::default()
            },
            BTreeMap::from([(
                LX_SOURCE_KG.to_owned(),
                lx_music_source(
                    LX_SOURCE_KG,
                    "Kugou",
                    vec![SourceAction::MusicUrl],
                    vec![SourceQuality::K128],
                ),
            )]),
        )
    }

    fn dispatch(
        runtime: &SourceRuntime,
        provider: &ImportedLxJsProvider,
    ) -> Result<SourceRequestOutcome, SourceRuntimeError> {
        runtime.initialize_provider(provider)?;
        runtime.dispatch_request(
            provider,
            SourceRequest::MusicUrl {
                source: LX_SOURCE_KG.to_owned(),
                music_info: json!({ "id": "track-hash", "albumId": "album-1" }),
                quality: SourceQuality::K128,
            },
        )
    }

    #[test]
    fn imported_script_should_resolve_music_url_through_host_network() {
        let host = Arc::new(RecordingHost::default());
        let runtime = SourceRuntime::with_host(host.clone(), [SourceCapability::NetworkAny]);
        let outcome = dispatch(&runtime, &provider(TEST_SOURCE))
            .expect("LX JavaScript should resolve a music URL");

        assert_eq!(
            outcome.response,
            SourceResponse::MusicUrl("https://cdn.example.test/song.mp3".to_owned())
        );
        let requests = host.requests.lock().expect("request log should lock");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, SourceHttpMethod::Post);
        assert_eq!(requests[0].headers["x-source"], LX_SOURCE_KG);
        assert_eq!(
            serde_json::from_slice::<JsonValue>(requests[0].body.as_deref().unwrap_or_default())
                .expect("request body should be JSON"),
            json!({ "id": "track-hash", "quality": "128k" })
        );
        assert_eq!(requests[0].timeout, Some(Duration::from_millis(2500)));
    }

    #[test]
    fn imported_script_should_upgrade_http_media_redirects_for_webview_playback() {
        let source = r#"
            const { EVENT_NAMES, on, send } = globalThis.lx;
            on(EVENT_NAMES.request, () => Promise.resolve('http://resolver.example.test/play'));
            send(EVENT_NAMES.inited, { sources: { kg: { type: 'music', actions: ['musicUrl'], qualitys: ['128k'] } } });
        "#;
        let host = Arc::new(RedirectingMediaHost::default());
        let runtime = SourceRuntime::with_host(host.clone(), [SourceCapability::NetworkAny]);
        let outcome = dispatch(&runtime, &provider(source))
            .expect("HTTP media redirect should be upgraded through the source host");

        assert_eq!(
            outcome.response,
            SourceResponse::MusicUrl("https://media.example.test/song.flac".to_owned())
        );
        let requests = host.requests.lock().expect("request log should lock");
        assert_eq!(requests.len(), 2);
        assert!(requests
            .iter()
            .all(|request| request.method == SourceHttpMethod::Head));
    }

    #[test]
    fn imported_script_should_not_receive_node_dom_or_filesystem_apis() {
        let source = r#"
            const { EVENT_NAMES, on, send } = globalThis.lx;
            on(EVENT_NAMES.request, () => Promise.resolve(
              typeof require === 'undefined' &&
              typeof process === 'undefined' &&
              typeof document === 'undefined' &&
              typeof fetch === 'undefined'
                ? 'https://cdn.example.test/sandboxed.mp3'
                : 'file:///tmp/leak.mp3'
            ));
            send(EVENT_NAMES.inited, { sources: { kg: { type: 'music', actions: ['musicUrl'], qualitys: ['128k'] } } });
        "#;
        let runtime = SourceRuntime::with_granted_capabilities([SourceCapability::NetworkAny]);
        let outcome = dispatch(&runtime, &provider(source))
            .expect("sandboxed script should resolve through the LX contract");

        assert_eq!(
            outcome.response,
            SourceResponse::MusicUrl("https://cdn.example.test/sandboxed.mp3".to_owned())
        );
    }

    #[test]
    fn imported_script_should_be_interrupted_at_the_execution_limit() {
        let source = r#"
            const { EVENT_NAMES, on, send } = globalThis.lx;
            on(EVENT_NAMES.request, () => Promise.resolve('https://cdn.example.test/song.mp3'));
            send(EVENT_NAMES.inited, { sources: { kg: { type: 'music', actions: ['musicUrl'], qualitys: ['128k'] } } });
            while (true) {}
        "#;
        let provider = provider(source).with_execution_timeout(Duration::from_millis(25));
        let runtime = SourceRuntime::with_granted_capabilities([SourceCapability::NetworkAny]);
        let error = runtime
            .initialize_provider(&provider)
            .expect_err("infinite JavaScript should be interrupted");

        assert!(error.to_string().contains("execution limit"));
    }

    #[test]
    fn imported_script_should_stop_when_the_source_request_is_cancelled() {
        let source = r#"
            const { EVENT_NAMES, on, send } = globalThis.lx;
            on(EVENT_NAMES.request, () => { while (true) {} });
            send(EVENT_NAMES.inited, { sources: { kg: { type: 'music', actions: ['musicUrl'], qualitys: ['128k'] } } });
        "#;
        let provider = Arc::new(provider(source));
        let runtime = Arc::new(SourceRuntime::with_granted_capabilities([
            SourceCapability::NetworkAny,
        ]));
        runtime
            .initialize_provider(provider.as_ref())
            .expect("source should initialize before its request loops");
        let cancellation = SourceCancellationToken::default();
        let request_cancellation = cancellation.clone();
        let request_runtime = Arc::clone(&runtime);
        let request_provider = Arc::clone(&provider);
        let request = std::thread::spawn(move || {
            request_runtime.dispatch_request_with_cancellation(
                request_provider.as_ref(),
                SourceRequest::MusicUrl {
                    source: LX_SOURCE_KG.to_owned(),
                    music_info: json!({ "id": "track-hash" }),
                    quality: SourceQuality::K128,
                },
                request_cancellation,
            )
        });
        std::thread::sleep(Duration::from_millis(25));
        cancellation.cancel();
        let error = request
            .join()
            .expect("cancelled JavaScript request should not panic")
            .expect_err("cancelled JavaScript request should stop");

        assert!(matches!(error, SourceRuntimeError::Cancelled { .. }));
    }

    #[test]
    fn lx_crypto_and_buffer_helpers_should_match_documented_basics() {
        let source = r#"
            const { EVENT_NAMES, on, send, utils } = globalThis.lx;
            on(EVENT_NAMES.request, () => {
              const bytes = utils.buffer.from('hello', 'utf8');
              const encrypted = utils.crypto.aesEncrypt(
                bytes,
                'aes-128-cbc',
                utils.buffer.from('1234567890abcdef', 'utf8'),
                utils.buffer.from('abcdef1234567890', 'utf8'),
              );
              const valid = utils.buffer.bufToString(bytes, 'hex') === '68656c6c6f'
                && Buffer.isBuffer(bytes)
                && Buffer.from('hello').toString('hex') === '68656c6c6f'
                && utils.crypto.md5('hello') === '5d41402abc4b2a76b9719d911017c592'
                && utils.crypto.randomBytes(8).length === 8
                && encrypted.toString('hex') === '9479a14122e3ff7cbbb64a120818709b';
              return Promise.resolve(valid
                ? 'https://cdn.example.test/helpers.mp3'
                : 'file:///invalid.mp3');
            });
            send(EVENT_NAMES.inited, { sources: { kg: { type: 'music', actions: ['musicUrl'], qualitys: ['128k'] } } });
        "#;
        let runtime = SourceRuntime::with_granted_capabilities([SourceCapability::NetworkAny]);
        let outcome = dispatch(&runtime, &provider(source))
            .expect("documented LX helpers should be available");

        assert_eq!(
            outcome.response,
            SourceResponse::MusicUrl("https://cdn.example.test/helpers.mp3".to_owned())
        );
    }

    #[test]
    fn rsa_helper_should_encrypt_with_a_pem_public_key() {
        use rsa::pkcs8::{EncodePublicKey, LineEnding};
        use rsa::RsaPrivateKey;

        let private_key =
            RsaPrivateKey::new(&mut OsRng, 1024).expect("test RSA key should generate");
        let public_key = RsaPublicKey::from(&private_key)
            .to_public_key_pem(LineEnding::LF)
            .expect("test RSA public key should encode");
        let encrypted = rsa_encrypt(&BASE64.encode(b"hello"), &public_key)
            .expect("LX RSA helper should encrypt");
        let decrypted = private_key
            .decrypt(Pkcs1v15Encrypt, &encrypted)
            .expect("test RSA private key should decrypt");

        assert_eq!(decrypted, b"hello");
    }

    #[test]
    fn script_messages_should_redact_url_query_parameters() {
        let message = sanitize_script_message(
            "request failed (https://api.example.test/play?token=secret&id=42) retrying",
        );

        assert!(message.contains("https://api.example.test/play?<redacted>"));
        assert!(!message.contains("secret"));
    }

    #[test]
    fn imported_script_should_require_a_valid_inited_catalog() {
        let source = r#"
            const { EVENT_NAMES, on, send } = globalThis.lx;
            on(EVENT_NAMES.request, () => Promise.resolve('https://cdn.example.test/song.mp3'));
            send(EVENT_NAMES.inited, {});
        "#;
        let runtime = SourceRuntime::with_granted_capabilities([SourceCapability::NetworkAny]);
        let error = runtime
            .initialize_provider(&provider(source))
            .expect_err("invalid inited payload should reject initialization");

        assert!(matches!(error, SourceRuntimeError::Provider { .. }));
        assert!(error.to_string().contains("non-empty sources object"));
    }

    #[test]
    fn imported_script_should_remain_usable_when_source_throws_after_inited_event() {
        let source = r#"
            const { EVENT_NAMES, on, send } = globalThis.lx;
            on(EVENT_NAMES.request, () => Promise.resolve('https://cdn.example.test/song.mp3'));
            send(EVENT_NAMES.inited, { sources: { kg: { type: 'music', actions: ['musicUrl'], qualitys: ['128k'] } } });
            throw new Error('post-inited source error');
        "#;
        let runtime = SourceRuntime::with_granted_capabilities([SourceCapability::NetworkAny]);
        let outcome = dispatch(&runtime, &provider(source))
            .expect("an error after the inited event should not disable the LX source");

        assert_eq!(
            outcome.response,
            SourceResponse::MusicUrl("https://cdn.example.test/song.mp3".to_owned())
        );
    }

    #[test]
    fn arithmetic_obfuscated_script_should_dispatch_music_url() {
        let source = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("fixtures/lx-js-sources/arithmetic-obfuscated-v1.0.0.js"),
        )
        .expect("arithmetic-obfuscated source should be readable");
        let runtime = SourceRuntime::with_granted_capabilities([SourceCapability::NetworkAny]);
        let outcome = dispatch(&runtime, &provider(&source))
            .expect("arithmetic-obfuscated source should dispatch in QuickJS");

        assert_eq!(
            outcome.response,
            SourceResponse::MusicUrl("https://cdn.example.test/song.mp3".to_owned())
        );
    }

    #[test]
    fn standalone_nianxin_and_changqing_scripts_should_use_their_own_kugou_endpoints() {
        let fixtures = [
            ("nianxin-v1.0.1.js", "mcp.nianxinxz.com/share/ceshi/kg.php"),
            ("changqing-svip-v1.2.0.js", "music.haitangw.cc/kgqq1/kg.php"),
        ];
        for (file_name, expected_endpoint) in fixtures {
            let source = std::fs::read_to_string(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("fixtures/lx-js-sources")
                    .join(file_name),
            )
            .expect("LX fixture should be readable");
            let report = crate::lx_js_importer::analyze_lx_js_source(file_name, &source)
                .expect("LX fixture should analyze");
            let source_catalog = report.manifest.to_source_catalog();
            let provider = ImportedLxJsProvider::new(
                format!("test-{file_name}"),
                report.manifest.display_name,
                source,
                report.metadata,
                source_catalog,
            );
            let host = Arc::new(RecordingHost::default());
            let runtime = SourceRuntime::with_host(host, [SourceCapability::NetworkAny]);
            runtime
                .initialize_provider(&provider)
                .expect("standalone source should initialize in QuickJS");
            let outcome = runtime
                .dispatch_request(
                    &provider,
                    SourceRequest::MusicUrl {
                        source: LX_SOURCE_KG.to_owned(),
                        music_info: json!({
                            "id": "04DE99837D367481C2CD07C107003E1D",
                            "hash": "04DE99837D367481C2CD07C107003E1D",
                            "songmid": "04DE99837D367481C2CD07C107003E1D",
                        }),
                        quality: SourceQuality::K128,
                    },
                )
                .expect("standalone source should return its script-defined URL");
            let SourceResponse::MusicUrl(url) = outcome.response else {
                panic!("standalone source should return musicUrl");
            };
            assert!(url.contains(expected_endpoint), "{file_name}: {url}");
        }
    }

    #[test]
    #[ignore = "requires FIKA_LX_JS_LIVE_SOURCE and live third-party endpoints"]
    fn live_external_lx_script_should_resolve_a_playable_kugou_url() {
        let source_path = std::env::var("FIKA_LX_JS_LIVE_SOURCE")
            .expect("FIKA_LX_JS_LIVE_SOURCE should point to an LX JavaScript file");
        let track_hash = std::env::var("FIKA_LX_JS_LIVE_HASH")
            .unwrap_or_else(|_| "04DE99837D367481C2CD07C107003E1D".to_owned());
        let quality = std::env::var("FIKA_LX_JS_LIVE_QUALITY")
            .ok()
            .and_then(|quality| SourceQuality::from_lx_str(&quality))
            .unwrap_or(SourceQuality::K128);
        let source = std::fs::read_to_string(&source_path)
            .expect("live LX JavaScript source should be readable");
        let report = crate::lx_js_importer::analyze_lx_js_source(&source_path, &source)
            .expect("live LX JavaScript source should analyze");
        let source_catalog = report.manifest.to_source_catalog();
        let provider = ImportedLxJsProvider::new(
            "live-lx-provider",
            report.manifest.display_name,
            source,
            report.metadata,
            source_catalog,
        );
        let runtime = SourceRuntime::with_granted_capabilities([SourceCapability::NetworkAny]);
        runtime
            .initialize_provider(&provider)
            .expect("live LX JavaScript source should initialize");
        let outcome = runtime
            .dispatch_request(
                &provider,
                SourceRequest::MusicUrl {
                    source: LX_SOURCE_KG.to_owned(),
                    music_info: json!({
                        "id": track_hash,
                        "hash": track_hash,
                        "songmid": track_hash,
                        "name": "无烟区",
                        "singer": "陈粒",
                        "interval": 322,
                    }),
                    quality,
                },
            )
            .expect("live LX JavaScript source should resolve KuGou playback");
        let SourceResponse::MusicUrl(url) = outcome.response else {
            panic!("live LX source should return musicUrl");
        };
        assert_eq!(
            url::Url::parse(&url)
                .expect("resolved musicUrl should remain valid")
                .scheme(),
            "https",
            "resolved musicUrl must be secure for WebView playback"
        );
        let mut response = reqwest::blocking::Client::new()
            .get(&url)
            .header(reqwest::header::RANGE, "bytes=0-1023")
            .timeout(Duration::from_secs(15))
            .send()
            .expect("resolved URL should accept a range request");

        assert!(response.status().is_success());
        let has_audio_content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("audio/"));
        let mut prefix = [0_u8; 12];
        let prefix_len = response
            .read(&mut prefix)
            .expect("resolved response should expose audio bytes");
        let prefix = &prefix[..prefix_len];
        let has_audio_signature = prefix.starts_with(b"fLaC")
            || prefix.starts_with(b"ID3")
            || prefix.starts_with(b"OggS")
            || (prefix.starts_with(b"RIFF") && prefix.get(8..12) == Some(b"WAVE"))
            || prefix.get(4..8) == Some(b"ftyp")
            || prefix
                .get(..2)
                .is_some_and(|bytes| bytes[0] == 0xff && bytes[1] & 0xe0 == 0xe0);
        assert!(has_audio_content_type || has_audio_signature);
    }
}
