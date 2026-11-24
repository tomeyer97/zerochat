import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// The zerochat server (cargo run / ./target/debug/zerochat) answers /api on this address:
// `ZEROCHAT_BACKEND`, else the server's default listen address (`--listen`/`ZEROCHAT_LISTEN` in
// crates/zerochat/src/port.rs). Change both together, e.g.
// `ZEROCHAT_LISTEN=127.0.0.1:4400 cargo run` and `ZEROCHAT_BACKEND=http://127.0.0.1:4400 npm run dev`.
const backend = process.env.ZEROCHAT_BACKEND ?? 'http://127.0.0.1:4380';

export default defineConfig({
  // Absolute asset paths (`/assets/…`): the server embeds dist/ and serves it at `/`, and answers
  // any other path (e.g. /runs/local/<id>) with index.html, whose assets must load from there too.
  // zerochat is never served under a path prefix (the API is fetched from `/api`).
  base: '/',
  plugins: [react()],
  build: { outDir: 'dist', emptyOutDir: true, chunkSizeWarningLimit: 2500 },
  server: {
    host: '127.0.0.1',
    port: 5174,
    proxy: {
      '/api': {
        target: backend,
        // Rewrites Host so the backend's DNS-rebinding check accepts the request.
        changeOrigin: true,
        configure(proxy) {
          proxy.on('proxyReq', (request, incoming) => {
            // The backend only accepts state changes from its own origin; adapt the Vite origin.
            if (incoming.headers.origin) request.setHeader('origin', new URL(backend).origin);
          });
        },
      },
    },
  },
});
