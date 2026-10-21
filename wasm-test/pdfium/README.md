# pdfium (wasm)

The test page's `compare` option renders PDFs with the wasm build from
<https://github.com/bblanchon/pdfium-binaries> (the same project the native
`external/pdfium` libraries come from). The binaries are not tracked; fetch them with:

```sh
cd wasm-test/pdfium
curl -sL https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-wasm.tgz | tar xz
```

This yields `lib/pdfium.js` + `lib/pdfium.wasm` (the page loads them lazily on the
first compare run), plus `LICENSE` and `licenses/`.
