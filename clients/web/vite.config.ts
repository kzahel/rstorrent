import { defineConfig, type ProxyOptions } from "vite";

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

export default defineConfig({
  server: { proxy },
  preview: { proxy },
});
