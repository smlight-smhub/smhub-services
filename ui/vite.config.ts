import { createHash } from 'node:crypto';
import { relative } from 'node:path';
import { defineConfig, type Plugin } from 'vite';

// The hub (or the daemon) that `npm run dev` and `npm run preview` forward
// /api and /ws to. Only the development and test tools use it.
const hub = process.env.SMHUB_URL ?? 'http://127.0.0.1:8000';
const hubOrigin = new URL(hub).origin;
// The daemon refuses writes whose Origin is another site. Through this proxy
// the browser's origin is localhost, so the forwarded request carries the
// hub's own origin, as it would when the hub serves the UI itself.
type ProxyServer = { on(event: string, fn: (req: { setHeader(name: string, value: string): void }) => void): void };
const sameOrigin = (proxy: ProxyServer) => {
  proxy.on('proxyReq', (req) => req.setHeader('origin', hubOrigin));
  proxy.on('proxyReqWs', (req) => req.setHeader('origin', hubOrigin));
};
const proxy = {
  '/api': { target: hub, changeOrigin: true, configure: sameOrigin },
  '/ws': { target: hub, ws: true, changeOrigin: true, configure: sameOrigin },
};

// D43: a strict content security policy. nginx on the hub is not changed, so
// it is set by a meta tag, in the built page only (the dev server injects
// styles inline). Styles allow 'unsafe-inline' because the terminal (xterm)
// creates <style> elements; scripts come only from the hub.
const csp = [
  "default-src 'self'",
  "script-src 'self'",
  "style-src 'self' 'unsafe-inline'",
  "img-src 'self' data: blob:",
  "connect-src 'self'",
  'frame-src http: https:',
  "object-src 'none'",
  "base-uri 'self'",
  "form-action 'self'",
].join('; ');

function contentSecurityPolicy(): Plugin {
  return {
    name: 'smhub-csp',
    apply: 'build',
    transformIndexHtml: () => [
      { tag: 'meta', attrs: { 'http-equiv': 'Content-Security-Policy', content: csp }, injectTo: 'head-prepend' },
    ],
  };
}

export default defineConfig(({ command }) => ({
  plugins: [contentSecurityPolicy()],
  esbuild: { jsx: 'automatic', jsxImportSource: 'preact', legalComments: 'none' },
  css: {
    modules: {
      // Short, stable class names in the build; readable ones in development.
      generateScopedName(local: string, file: string) {
        const rel = relative(process.cwd(), file).split('\\').join('/');
        if (command === 'serve') {
          const base = rel.split('/').pop()?.replace(/\.module\.css$/, '') ?? 'c';
          return `${base}_${local}`;
        }
        return '_' + createHash('sha1').update(rel + ':' + local).digest('base64url').slice(0, 6);
      },
    },
  },
  build: {
    target: 'es2022',
    outDir: 'dist',
    assetsDir: 'assets',
    modulePreload: { polyfill: false },
    reportCompressedSize: false,
    sourcemap: false,
    chunkSizeWarningLimit: 400,
    rollupOptions: {
      // Icons and helpers shared by two pages would otherwise become many
      // tiny files; each request costs the hub CPU, so small ones are merged.
      output: { experimentalMinChunkSize: 6000 },
    },
  },
  server: { port: 5173, proxy },
  preview: { port: 4173, proxy },
}));
