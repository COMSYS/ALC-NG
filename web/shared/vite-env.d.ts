/// <reference types="vite/client" />

interface ImportMetaEnv {
  // URL of alc_ng.wasm when it is not served from the default cargo target path
  readonly VITE_ALC_NG_WASM?: string
  // where pdf.js finds its CMaps, standard fonts and decoders (vite.config.ts)
  readonly PDFJS_BASE: string
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}
