const denoRuntime = globalThis.Deno;
const nodeCrypto = await import('node:crypto');
const { Buffer: NodeBuffer } = await import('node:buffer');
const encoder = new TextEncoder();
const decoder = new TextDecoder();
const stdinReader = denoRuntime.stdin.readable.getReader();
const stdoutWriter = denoRuntime.stdout.writable.getWriter();

let inputBuffer = '';
let writeQueue = Promise.resolve();

async function readLine() {
  while (true) {
    const newline = inputBuffer.indexOf('\n');
    if (newline !== -1) {
      const line = inputBuffer.slice(0, newline);
      inputBuffer = inputBuffer.slice(newline + 1);
      return line;
    }
    const { value, done } = await stdinReader.read();
    if (done) {
      if (!inputBuffer) return null;
      const line = inputBuffer;
      inputBuffer = '';
      return line;
    }
    inputBuffer += decoder.decode(value, { stream: true });
  }
}

const commandLine = await readLine();
if (!commandLine) throw new Error('missing V8 sidecar command');
const command = JSON.parse(commandLine);
const nonce = String(command.nonce || '');
if (!nonce) throw new Error('missing V8 sidecar nonce');

function emit(message) {
  const encoded = encoder.encode(`${JSON.stringify({ nonce, ...message })}\n`);
  writeQueue = writeQueue.then(() => stdoutWriter.write(encoded));
  return writeQueue;
}

function errorMessage(error) {
  if (error && typeof error.message === 'string') return error.message;
  try { return String(error); } catch (_) { return 'unknown V8 sidecar error'; }
}

function sanitizeLogValue(value) {
  if (value instanceof Error) return value.stack || value.message;
  if (typeof value === 'string') return value;
  try {
    const encoded = JSON.stringify(value);
    return encoded === undefined ? String(value) : encoded;
  } catch (_) {
    try { return String(value); } catch (_) { return '<unprintable>'; }
  }
}

const pendingRequests = new Map();
let nextRequestId = 1;

async function pumpResponses() {
  while (true) {
    const line = await readLine();
    if (line === null) return;
    let message;
    try { message = JSON.parse(line); } catch (_) { continue; }
    if (message.nonce !== nonce || message.type !== 'httpResponse') continue;
    const pending = pendingRequests.get(String(message.id));
    if (!pending) continue;
    pendingRequests.delete(String(message.id));
    if (!message.ok) {
      pending.callback(new Error(String(message.error || 'LX network request failed')), null, null);
      continue;
    }
    const response = message.response || {};
    let body = typeof response.body === 'string' ? response.body : '';
    try { body = JSON.parse(body); } catch (_) {}
    const normalized = {
      statusCode: Number(response.statusCode || 0),
      headers: response.headers || {},
      body,
    };
    pending.callback(null, normalized, body);
  }
}

void pumpResponses();

const handlers = Object.create(null);
const EVENT_NAMES = Object.freeze({
  inited: 'inited',
  request: 'request',
  updateAlert: 'updateAlert',
});
let initialized = false;
let initializedCatalog = null;
let resolveInitialized;
const initializedPromise = new Promise(resolve => { resolveInitialized = resolve; });
const nativeSetTimeout = globalThis.setTimeout;

function request(url, options, callback) {
  if (typeof callback !== 'function') throw new TypeError('LX request callback is required');
  const id = String(nextRequestId++);
  pendingRequests.set(id, { callback });
  void emit({
    type: 'httpRequest',
    id,
    url: String(url),
    options: options || {},
  });
  return () => pendingRequests.delete(id);
}

function on(eventName, handler) {
  if (typeof eventName !== 'string' || typeof handler !== 'function') {
    return Promise.reject(new TypeError('LX event registration requires a name and handler'));
  }
  handlers[eventName] = handler;
  return Promise.resolve();
}

function send(eventName, data) {
  if (eventName === EVENT_NAMES.inited) {
    const sources = data && typeof data === 'object' && !Array.isArray(data) ? data.sources : null;
    if (!sources || typeof sources !== 'object' || Array.isArray(sources) || Object.keys(sources).length === 0) {
      return Promise.reject(new TypeError('LX inited event requires a non-empty sources object'));
    }
    initialized = true;
    initializedCatalog = sources;
    resolveInitialized();
  } else if (eventName === EVENT_NAMES.updateAlert) {
    void emit({ type: 'log', level: 'warn', message: sanitizeLogValue(data?.log || 'source update is available') });
  }
  return Promise.resolve();
}

const buffer = Object.freeze({
  from(value, format = 'utf8') { return NodeBuffer.from(value, format); },
  bufToString(value, format = 'utf8') { return NodeBuffer.from(value).toString(format); },
});

const crypto = Object.freeze({
  md5(value) { return nodeCrypto.createHash('md5').update(String(value)).digest('hex'); },
  randomBytes(size) { return nodeCrypto.randomBytes(Number(size)); },
  aesEncrypt(value, mode, key, iv) {
    const cipher = nodeCrypto.createCipheriv(String(mode), NodeBuffer.from(key), iv == null ? null : NodeBuffer.from(iv));
    return NodeBuffer.concat([cipher.update(NodeBuffer.from(value)), cipher.final()]);
  },
  rsaEncrypt(value, key) {
    const input = NodeBuffer.from(value);
    if (input.length > 128) throw new Error('RSA input exceeds 128 bytes');
    const padded = NodeBuffer.concat([NodeBuffer.alloc(128 - input.length), input]);
    return nodeCrypto.publicEncrypt({
      key: String(key),
      padding: nodeCrypto.constants.RSA_NO_PADDING,
    }, padded);
  },
});

const scriptInfo = Object.freeze({
  ...(command.scriptInfo || {}),
  rawScript: String(command.source || ''),
});
const lx = Object.freeze({
  version: '2.0.0',
  env: 'desktop',
  currentScriptInfo: scriptInfo,
  EVENT_NAMES,
  on,
  send,
  request,
  utils: Object.freeze({ buffer, crypto }),
});

Object.defineProperty(globalThis, 'lx', { value: lx, configurable: false });
Object.defineProperty(globalThis, 'Buffer', {
  value: Object.freeze({ from: NodeBuffer.from, isBuffer: NodeBuffer.isBuffer }),
  configurable: false,
});
Object.defineProperty(globalThis, 'console', {
  value: Object.freeze({
    log: (...args) => void emit({ type: 'log', level: 'info', message: args.map(sanitizeLogValue).join(' ') }),
    info: (...args) => void emit({ type: 'log', level: 'info', message: args.map(sanitizeLogValue).join(' ') }),
    warn: (...args) => void emit({ type: 'log', level: 'warn', message: args.map(sanitizeLogValue).join(' ') }),
    error: (...args) => void emit({ type: 'log', level: 'error', message: args.map(sanitizeLogValue).join(' ') }),
    debug: () => {},
  }),
  configurable: false,
});

for (const name of [
  'Deno',
  'process',
  'fetch',
  'WebSocket',
  'Worker',
  'SharedWorker',
  'EventSource',
  'BroadcastChannel',
  'localStorage',
  'sessionStorage',
]) {
  try { delete globalThis[name]; } catch (_) {
    try { globalThis[name] = undefined; } catch (_) {}
  }
}

let sourceError = null;
try {
  (0, eval)(String(command.source || ''));
} catch (error) {
  sourceError = error;
}
await Promise.resolve();

if (!initialized) {
  await Promise.race([
    initializedPromise,
    new Promise(resolve => nativeSetTimeout(resolve, 10_000)),
  ]);
}
if (!initialized || typeof handlers[EVENT_NAMES.request] !== 'function') {
  throw sourceError || new Error('source.js did not complete the LX inited/request contract');
}
if (sourceError) {
  void emit({ type: 'log', level: 'warn', message: `LX source error after initialization: ${errorMessage(sourceError)}` });
}

let value = null;
if (command.payload != null) value = await handlers[EVENT_NAMES.request](command.payload);
await emit({ type: 'complete', ok: true, catalog: initializedCatalog, value });
await writeQueue;
await stdinReader.cancel();
await stdoutWriter.close();
denoRuntime.exit(0);
