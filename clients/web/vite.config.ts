import { resolve } from "node:path";
import { defineConfig, loadEnv, type ProxyOptions } from "vite";

const proxyTarget = process.env.RSTORRENT_WEBUI_PROXY_TARGET;
const proxy: Record<string, string | ProxyOptions> | undefined =
  proxyTarget === undefined
    ? undefined
    : {
        "/api": {
          target: proxyTarget,
          changeOrigin: true,
          ws: true,
        },
      };

export default defineConfig(({ mode }) => {
  const remote = mode === "remote";
  const companion = mode === "companion";
  if (remote) {
    const environment = loadEnv(mode, process.cwd(), "");
    const relay = environment.VITE_RSTORRENT_REMOTE_RELAY_URL;
    const build = environment.VITE_RSTORRENT_REMOTE_BUILD_ID;
    if (relay === undefined || !relay.startsWith("wss://")) {
      throw new Error(
        "remote build requires VITE_RSTORRENT_REMOTE_RELAY_URL=wss://...",
      );
    }
    if (build === undefined || build.length < 1 || build.length > 160) {
      throw new Error("remote build requires VITE_RSTORRENT_REMOTE_BUILD_ID");
    }
  }
  return {
    server: { proxy },
    preview: { proxy },
    ...(companion
      ? {
          base: "./",
          publicDir: false,
          build: {
            outDir: "dist/companion",
            emptyOutDir: true,
            rollupOptions: {
              input: resolve(process.cwd(), "companion.html"),
              output: {
                entryFileNames: "assets/companion.js",
                chunkFileNames: "assets/[name].js",
                assetFileNames: "assets/companion.[ext]",
              },
            },
          },
        }
      : {}),
    ...(remote
      ? {
          base: "/remote/",
          publicDir: false,
          resolve: {
            alias: {
              "rstorrent-remote-wasm-client": resolve(
                process.cwd(),
                ".remote-wasm/rstorrent_remote_wasm.js",
              ),
            },
          },
          build: {
            outDir: "dist/remote",
            rollupOptions: {
              input: resolve(process.cwd(), "remote.html"),
            },
          },
        }
      : {}),
  };
});
