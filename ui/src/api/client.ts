// The one request function (D30). Paths, parameters, bodies and answers are
// typed from the generated contract, so a call that does not match
// `api/openapi.yaml` fails the type check.
import type { paths } from './schema';

export const API_BASE = '/api/v2';

type Method = 'get' | 'post' | 'put' | 'patch' | 'delete';
type Op<P extends keyof paths, M extends Method> = NonNullable<paths[P][M]>;

/** Paths that have the given method. */
export type PathsWith<M extends Method> = {
  [P in keyof paths]: paths[P][M] extends undefined ? never : P;
}[keyof paths];

type ContentOf<R> = R extends { content: { 'application/json': infer J } }
  ? J
  : R extends { content: { 'text/markdown': infer T } }
    ? T
    : void;
type SuccessCode = 200 | 201 | 202 | 204;

/** What a successful call answers, from the contract. */
export type Answer<P extends keyof paths, M extends Method> =
  Op<P, M> extends { responses: infer R } ? ContentOf<R[Extract<keyof R, SuccessCode>]> : never;

type BodyOf<O> = O extends { requestBody?: never }
  ? undefined
  : O extends { requestBody?: { content: { 'application/json': infer B } } }
    ? B
    : O extends { requestBody?: { content: { 'application/octet-stream': unknown } } }
      ? Blob | string
      : undefined;
type PathParamsOf<O> = O extends { parameters: { path: infer PP } } ? PP : never;
type QueryOf<O> = O extends { parameters: { query?: infer Q } } ? Q : never;

export type CallOptions<O> = ([PathParamsOf<O>] extends [never]
  ? { path?: undefined }
  : { path: PathParamsOf<O> }) & {
  query?: QueryOf<O>;
  body?: BodyOf<O>;
  signal?: AbortSignal;
  /** Do not show the sign-in screen when this call answers 401. */
  quiet?: boolean;
};

/** An error answer from the hub, in the contract's one shape (D8). */
export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
    readonly field: string | null = null,
  ) {
    super(message);
    this.name = 'ApiError';
  }
}

let unauthorizedHandler: (() => void) | undefined;
/** Called once by the session state: what to do when the hub answers 401. */
export function onUnauthorized(handler: () => void) {
  unauthorizedHandler = handler;
}

/** Fills `{name}` placeholders and adds the query string. */
export function buildUrl(path: string, params?: Record<string, unknown>, query?: Record<string, unknown>) {
  let url =
    API_BASE +
    path.replace(/\{(\w+)\}/g, (_, key: string) => encodeURIComponent(String(params?.[key] ?? '')));
  if (query) {
    const q = new URLSearchParams();
    for (const [k, v] of Object.entries(query)) if (v !== undefined && v !== null && v !== '') q.set(k, String(v));
    const s = q.toString();
    if (s) url += '?' + s;
  }
  return url;
}

interface RawInit {
  body?: unknown;
  signal?: AbortSignal;
  quiet?: boolean;
  accept?: string;
}

/** Sends a request and maps every failure to an ApiError. Answers the Response. */
export async function request(method: string, url: string, init: RawInit = {}): Promise<Response> {
  const headers: Record<string, string> = { Accept: init.accept ?? 'application/json' };
  let body: BodyInit | undefined;
  if (init.body instanceof Blob || typeof init.body === 'string') {
    body = init.body;
    headers['Content-Type'] = 'application/octet-stream';
  } else if (init.body !== undefined) {
    body = JSON.stringify(init.body);
    headers['Content-Type'] = 'application/json';
  }
  let res: Response;
  try {
    res = await fetch(url, { method, headers, body, credentials: 'same-origin', signal: init.signal });
  } catch (e) {
    if ((e as Error).name === 'AbortError') throw e;
    throw new ApiError(0, 'network', (e as Error).message || 'Network error');
  }
  if (res.ok) return res;

  let code = res.status === 401 ? 'unauthorized' : res.status >= 502 ? 'unavailable' : 'internal';
  let message = res.statusText || `HTTP ${res.status}`;
  let field: string | null = null;
  if ((res.headers.get('content-type') ?? '').includes('json')) {
    try {
      const data = (await res.json()) as { error?: { code?: string; message?: string; field?: string | null } };
      if (data.error) {
        code = data.error.code || code;
        message = data.error.message || message;
        field = data.error.field ?? null;
      }
    } catch {
      /* not the contract's shape: keep the status-based code */
    }
  }
  const err = new ApiError(res.status, code, message, field);
  if (res.status === 401 && code !== 'auth_failed' && !init.quiet) unauthorizedHandler?.();
  throw err;
}

async function decode(res: Response): Promise<unknown> {
  if (res.status === 204) return undefined;
  const type = res.headers.get('content-type') ?? '';
  if (type.includes('json')) return res.json();
  if (type.startsWith('text/')) return res.text();
  const text = await res.text();
  if (!text) return undefined;
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}

function call<M extends Method, P extends PathsWith<M>>(
  method: M,
  path: P,
  opts?: CallOptions<Op<P, M>>,
): Promise<Answer<P, M>> {
  const o = (opts ?? {}) as { path?: Record<string, unknown>; query?: Record<string, unknown>; body?: unknown; signal?: AbortSignal; quiet?: boolean };
  const url = buildUrl(path, o.path, o.query);
  return request(method.toUpperCase(), url, {
    body: o.body,
    signal: o.signal,
    quiet: o.quiet,
    accept: path.endsWith('/changelog') ? 'text/markdown, text/plain, */*' : undefined,
  }).then(decode) as Promise<Answer<P, M>>;
}

export const api = {
  get: <P extends PathsWith<'get'>>(path: P, opts?: CallOptions<Op<P, 'get'>>) => call('get', path, opts),
  post: <P extends PathsWith<'post'>>(path: P, opts?: CallOptions<Op<P, 'post'>>) => call('post', path, opts),
  put: <P extends PathsWith<'put'>>(path: P, opts?: CallOptions<Op<P, 'put'>>) => call('put', path, opts),
  patch: <P extends PathsWith<'patch'>>(path: P, opts?: CallOptions<Op<P, 'patch'>>) => call('patch', path, opts),
  del: <P extends PathsWith<'delete'>>(path: P, opts?: CallOptions<Op<P, 'delete'>>) => call('delete', path, opts),
};

/** URL of a GET endpoint, for links, downloads and images. */
export function apiUrl<P extends PathsWith<'get'>>(path: P, params?: Record<string, unknown>, query?: Record<string, unknown>) {
  return buildUrl(path, params, query);
}

/** Address of a WebSocket endpoint on the same host. */
export function wsUrl(path: string) {
  return `${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}${path}`;
}
