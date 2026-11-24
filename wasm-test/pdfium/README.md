# pdfium (wasm)

The test page's `compare` option renders PDFs with the wasm build from
<https://github.com/bblanchon/pdfium-binaries> (the same project the native
`external/pdfium` libraries come from). The binaries are not tracked; fetch them with:

```sh
cd wasm-test/pdfium
curl -sSfL -o /tmp/pdfium-wasm.tgz \
    https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F8111/pdfium-wasm.tgz
echo "ed8ee2e70c628a4bd261fade8e381079aca250aeb5b1f3192c59682f8441fdcd  /tmp/pdfium-wasm.tgz" | shasum -a 256 -c -
tar xzf /tmp/pdfium-wasm.tgz
```

This is the release `Dockerfile.wasm` pins (`PDFIUM_VERSION`/`PDFIUM_SHA256`); update
both places together.

This yields `lib/pdfium.js` + `lib/pdfium.wasm` (the page loads them lazily on the
first compare run), plus `LICENSE` and `licenses/`.
