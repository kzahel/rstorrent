/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_RSTORRENT_INTEROP_MAGNET?: string;
  readonly VITE_RSTORRENT_INTEROP_GATEWAY_URL?: string;
  readonly VITE_RSTORRENT_INTEROP_GATEWAY_TOKEN?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
