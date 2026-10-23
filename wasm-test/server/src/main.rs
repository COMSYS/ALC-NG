//! Serves the repository for wasm-test/index.html and backs the engine's
//! texlive-on-demand requests with the local TeX Live installation.
//!
//! (Rust/axum port of the original Python `wasm-test/serve.py`; the wire
//! protocol towards the engine workers is unchanged.)
//!
//! The real SwiftLaTeX mirror (https://texlive2.swiftlatex.com/) is offline
//! and sent no CORS headers anyway. The engine workers fetch every kpathsea
//! file from `<endpoint><engine>/<format>/<name>` (engine `pdftex` or `xetex`;
//! the dvipdfmx worker uses `xetex`, PK requests arrive as
//! `pdftex/pk/<dpi>/<name>`); this server proxies GET `/texlive/<path>` as:
//!
//! 1. the per-URL cache under wasm-test/texlive-cache/ (gitignored),
//! 2. the local TeX Live resolved with `kpsewhich <basename>` for that engine
//!    (PK requests are answered only by real `.pk` files — never by extension
//!    fallbacks),
//! 3. a 301 — the mirror protocol's "file does not exist" marker, which the
//!    workers cache as a negative. The dead mirror fetch this used to end in
//!    is gone; Dockerfile.wasm runs the server inside the TeX Live full
//!    image, where step 2 answers the entire distribution.
//!
//! Map answers (format 11) additionally get the tracked fallback lines from
//! wasm-test/texlive-extras/<mapname> appended — the engine honours the last
//! map entry, so those override bitmap-only fonts the cached map cannot serve.
//!
//! Usage (from the repository root):
//!
//!     cargo run --release --manifest-path wasm-test/server/Cargo.toml -- [port]
//!
//! Then open:  http://localhost:<port>/
//! (The bench page is served at the site root; the former /wasm-test/
//! location forwards there, and all files stay served under their paths.)
//!
//! SERVE_ROOT=<dir> serves a different repository root, SERVE_HOST=0.0.0.0
//! exposes the server (e.g. inside a container), and SERVE_VERBOSE=1 logs
//! every /texlive/ request with its status.
//!
//! GET /tree/<dir> additionally lists a directory of the repository as JSON,
//! which is how the page fills a sample given as a folder (see SAMPLES in
//! index.html) without hardcoding every path of a submission.
//!
//! GET /fmt/<key>/<name> serves the prebuilt LaTeX format dumps: the page
//! reads one before it builds the format itself. Dumps are engine-build
//! specific — a native TeX Live .fmt cannot be used — so wasm-test/fmt-cache/
//! is populated at image build time (wasm-test/build-fmt.cjs) and served
//! read-only here. There is deliberately no upload route: a client-written
//! dump would be served to every browser.
//!
//! Cache policy: everything answers `no-cache` except plain TeX Live
//! packages — those bytes are frozen with the image, so their 200s carry a
//! one-day window and browser caches (and every re-run of a page session)
//! skip re-downloading the package set. Map files and format dumps opt out;
//! see `TEXLIVE_CACHE` and `get_fmt`.

use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, OnceLock},
    time::Duration,
};

use anyhow::{Context, Result};
use axum::{
    Router,
    body::Body,
    extract::{Path as UrlPath, Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::get,
};
use sha1::{Digest as _, Sha1};
use tower::ServiceBuilder;
use tower_http::{services::ServeDir, set_header::SetResponseHeaderLayer};

/// `kpsewhich` runs get this long before the attempt counts as a miss —
/// TeX filename lookups on a cold ls-RDB walk the tree, but never that far.
const KPSEWHICH_TIMEOUT: Duration = Duration::from_secs(60);

/// kpathsea format ids (kpse_file_format_type) of requests that arrive
/// without an extension: the extension to try first and/or the format name
/// `kpsewhich -format=` accepts as a last resort. One row per id so both
/// fallbacks are visible together — an id supported on only one fallback
/// (e.g. `11`/map) is intentional, not a missing entry.
const KPSE_FORMATS: &[(&str, Option<&'static str>, Option<&'static str>)] = &[
    ("3", Some(".tfm"), None),
    ("6", Some(".bib"), None),
    ("7", Some(".bst"), None),
    ("11", None, Some("map")),
    ("32", Some(".pfb"), Some("type1 fonts")),
    ("33", Some(".vf"), None),
    ("36", Some(".ttf"), Some("truetype fonts")),
    ("41", None, Some("misc fonts")),
    ("44", Some(".enc"), Some("enc files")),
    ("45", None, Some("cmap files")),
    ("46", None, Some("subfont definition files")),
    ("47", Some(".otf"), Some("opentype fonts")),
];

/// The extension to try first for a format id.
fn format_ext(fmt: &str) -> Option<&'static str> {
    KPSE_FORMATS
        .iter()
        .find(|(id, ..)| *id == fmt)
        .and_then(|(_, ext, _)| *ext)
}

/// ... and the format name kpsewhich `-format=` takes, as a last resort.
fn format_name(fmt: &str) -> Option<&'static str> {
    KPSE_FORMATS
        .iter()
        .find(|(id, ..)| *id == fmt)
        .and_then(|(_, _, name)| *name)
}

/// Whether the host actually has `kpsewhich`; probed once, lazily.
fn kpsewhich_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        std::process::Command::new("kpsewhich")
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    })
}

struct App {
    /// Repository root: everything below it is served statically.
    root: PathBuf,
    /// Per-URL cache of resolved TeX Live files (one flat file per request,
    /// slashes replaced by `__` — same layout the Python server left behind).
    cache: PathBuf,
    /// fileid -> original request, so dvipdfmx's `/tex/<fileid>` lookups
    /// find their way back (the engine never stores names, only this id).
    fileids: PathBuf,
    /// Tracked byte-level fallbacks per map file (see texlive-extras/pdftex.map).
    extras: PathBuf,
    /// Prebuilt LaTeX format dumps (`.fmt`), baked into the image by the
    /// Dockerfile's fmt-build stage (wasm-test/build-fmt.cjs). A dump is only
    /// valid for the exact wasm engine build that wrote it, so the page names
    /// it after its engine version key; kpsewhich is never consulted.
    ///
    /// Read-only on purpose: no client may write here. Users are untrusted,
    /// and a bad dump would poison every browser that fetches it.
    fmt_cache: PathBuf,
    verbose: bool,
}

/// The two dumps the patched engines look for (see wasm-test/swiftlatex).
const FMT_NAMES: [&str; 2] = ["swiftlatexpdftex.fmt", "swiftlatexxetex.fmt"];

/// Resolves a (version key, dump name) pair to its cache file, rejecting
/// anything outside the whitelist — the path comes straight from the URL.
fn fmt_cache_path(app: &App, key: &str, name: &str) -> Option<PathBuf> {
    let name_ok = FMT_NAMES.contains(&name);
    let key_ok = !key.is_empty()
        && key.len() <= 96
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    (name_ok && key_ok).then(|| app.fmt_cache.join(format!("{key}__{name}")))
}

/// Port, serve host, and configured state from the CLI argument and the
/// `SERVE_*` environment variables (see the module documentation).
fn configure() -> Result<(u16, String, Arc<App>)> {
    let port: u16 = env::args()
        .nth(1)
        .map(|arg| arg.parse())
        .transpose()
        .context("port must be a number")?
        .unwrap_or(8000);
    let host = env::var("SERVE_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let root = env::var_os("SERVE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().expect("no current directory"));
    let root = root
        .canonicalize()
        .context("serving root does not exist - run from the repository root or set SERVE_ROOT")?;

    let app = Arc::new(App {
        cache: root.join("wasm-test/texlive-cache"),
        fileids: root.join("wasm-test/texlive-cache/fileids"),
        extras: root.join("wasm-test/texlive-extras"),
        fmt_cache: root.join("wasm-test/fmt-cache"),
        root,
        verbose: env::var_os("SERVE_VERBOSE").is_some_and(|v| !v.is_empty()),
    });
    fs::create_dir_all(&app.fmt_cache)
        .with_context(|| format!("creating format cache dir {}", app.fmt_cache.display()))?;
    Ok((port, host, app))
}

/// Bind `host:port` — a bare IP directly, anything else (e.g. "localhost")
/// after resolution.
async fn bind(host: &str, port: u16) -> Result<tokio::net::TcpListener> {
    let addr = format!("{host}:{port}");
    let listener = match addr.parse::<std::net::SocketAddr>() {
        Ok(socket) => tokio::net::TcpListener::bind(socket).await,
        Err(_) => tokio::net::TcpListener::bind((host, port)).await,
    }
    .with_context(|| format!("cannot serve on {addr} (SERVE_HOST={host})"))?;
    Ok(listener)
}

/// The blanket `Cache-Control` for the page, its files and everything whose
/// answer is not frozen with the image (dumps, negatives, errors).
const NO_CACHE: &str = "no-cache";

/// Freshness window for TeX Live package bytes: they change only when a new
/// image is deployed, so one day after a deploy is the worst a browser can
/// keep an outdated file. Map files are excluded — `extend_map` below mixes
/// in the tracked texlive-extras/ overrides, so their bytes are not purely
/// frozen with the image.
const TEXLIVE_CACHE: &str = "public, max-age=86400";

/// All routes plus the static fallback.
///
/// The page, its files and the listings answer no-cache — a test server, so
/// edits take effect after a normal reload — while the engine's TeX Live
/// routes set their own `Cache-Control` per response (see `TEXLIVE_CACHE`),
/// which is why they cannot sit behind the blanket layer.
fn build_router(app: Arc<App>) -> Router {
    let no_cache = SetResponseHeaderLayer::overriding(
        header::CACHE_CONTROL,
        HeaderValue::from_static(NO_CACHE),
    );
    let static_files = ServiceBuilder::new()
        .layer(no_cache.clone())
        .service(ServeDir::new(&app.root));
    // Engine content, outside the no-cache layer: every response from
    // here states its cacheability itself.
    let engine_files = Router::new()
        .route(
            "/texlive/{*rel}",
            get(texlive).route_layer(middleware::from_fn_with_state(app.clone(), log_texlive)),
        )
        // Prebuilt LaTeX format dumps (read-only): the page fetches one
        // before it considers building the format itself, so a fresh
        // browser skips the multi-minute INITEX run.
        .route("/fmt/{key}/{name}", get(get_fmt));
    // The bench page lives at the site root; the former /wasm-test/
    // location forwards there. The files under wasm-test/ themselves
    // keep being served by the static fallback.
    let pages = Router::new()
        .route("/tree/{*dir}", get(tree))
        .route("/", get(index))
        .route("/wasm-test", get(moved_to_root))
        .route("/wasm-test/", get(moved_to_root))
        .layer(no_cache);
    Router::new()
        .merge(engine_files)
        .merge(pages)
        .fallback_service(static_files)
        .with_state(app)
}

#[tokio::main]
async fn main() -> Result<()> {
    let (port, host, app) = configure()?;
    println!("Serving {} on http://localhost:{port}", app.root.display());
    println!(
        "texlive proxy: cache -> kpsewhich{} (mirror offline)",
        if kpsewhich_available() {
            ""
        } else {
            " NOT FOUND"
        }
    );
    let listener = bind(&host, port).await?;
    axum::serve(listener, build_router(app)).await?;
    Ok(())
}

/// The bench page at the site root: read at request time, so edits show up
/// after a plain reload (the page and its files answer no-cache anyway).
async fn index(State(app): State<Arc<App>>) -> Response {
    match tokio::fs::read(app.root.join("wasm-test/index.html")).await {
        Ok(html) => Response::builder()
            .status(StatusCode::OK)
            .header(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            )
            .body(Body::from(html))
            .unwrap(),
        Err(err) => (
            StatusCode::NOT_FOUND,
            format!("no bench page to serve: wasm-test/index.html: {err}"),
        )
            .into_response(),
    }
}

/// The page moved from /wasm-test/ to the root; keep old links working.
/// Temporary, so a relocated page never sticks in a browser's cache.
async fn moved_to_root() -> Response {
    Redirect::temporary("/").into_response()
}

/// A prebuilt format dump, or 404 so the page builds one itself.
///
/// The dump stays `no-cache`: a fresh browser fetches it at most once —
/// success then stores it in IndexedDB — and the URL key is a
/// hand-maintained string rather than a content hash, so a freshness
/// window could serve a dump from before an engine rebuild. Even fully
/// uncached it costs at most one download per browser.
async fn get_fmt(
    State(app): State<Arc<App>>,
    UrlPath((key, name)): UrlPath<(String, String)>,
) -> Response {
    let Some(path) = fmt_cache_path(&app, &key, &name) else {
        return no_cache_error(StatusCode::NOT_FOUND, "unknown format dump".into());
    };
    match tokio::fs::read(&path).await {
        Ok(data) => octet_stream(data, NO_CACHE, &[]),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            no_cache_error(StatusCode::NOT_FOUND, "no dump cached yet".into())
        }
        Err(err) => no_cache_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("reading format dump: {err}"),
        ),
    }
}

/// Quiet by default; SERVE_VERBOSE=1 logs the engines' TeX Live requests.
async fn log_texlive(State(app): State<Arc<App>>, request: Request, next: Next) -> Response {
    let path = request.uri().path().to_string();
    let response = next.run(request).await;
    if app.verbose {
        eprintln!("{path} -> {}", response.status().as_u16());
    }
    response
}

async fn texlive(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    UrlPath(rel): UrlPath<String>,
) -> Response {
    match serve_texlive(&app, &rel).await {
        // The engine worker derives its /tex cache path from the fileid/pkid
        // headers — keep them stable per request so repeated fetches rewrite
        // the same file.
        Ok(Some((data, fileid))) => {
            // Maps stay out of the freshness window: their bytes mix in the
            // tracked texlive-extras/ overrides, not only the frozen image.
            let cache = if is_map_request(&rel) { NO_CACHE } else { TEXLIVE_CACHE };
            // A content ETag makes even the no-cache maps cheap: the browser
            // revalidates with a ~30-byte request instead of re-downloading
            // the whole file on every compile, while an edited extras override
            // still shows up on the very next load.
            let etag = format!("\"{}\"", sha1_hex(&data));
            if not_modified(&headers, &etag) {
                return (
                    StatusCode::NOT_MODIFIED,
                    [(header::ETAG, etag), (header::CACHE_CONTROL, cache.to_string())],
                )
                    .into_response();
            }
            octet_stream(
                data,
                cache,
                &[
                    ("etag", etag),
                    ("fileid", fileid.clone()),
                    ("pkid", fileid),
                ],
            )
        }
        Ok(None) => proxy_not_found(),
        // proxy trouble: report as upstream error
        Err(err) => no_cache_error(
            StatusCode::BAD_GATEWAY,
            format!("texlive proxy failed: {err:#}"),
        ),
    }
}

/// `sha1` hex digest, reused for the ETags and the fileids below.
fn sha1_hex(bytes: &[u8]) -> String {
    Sha1::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// `If-None-Match` as browsers send it: `*`, comma-separated candidates,
/// and weak tags matching strong ones (RFC 9110 §13.2.2).
fn not_modified(headers: &HeaderMap, etag: &str) -> bool {
    headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|given| {
            given.split(',').any(|candidate| {
                let candidate = candidate.trim();
                candidate == "*" || candidate == etag || candidate == format!("W/{etag}")
            })
        })
}

/// Resolve one engine request, `Ok(None)` standing for "no such file".
async fn serve_texlive(app: &App, rel: &str) -> Result<Option<(Vec<u8>, String)>> {
    // XeTeX writes the cache path of every OpenType/TrueType font it loaded
    // (`/tex/<fileid>`) into the XDV; dvipdfmx strips `/tex/` and asks for
    // `xetex/<format>/<fileid>[.otf|.ttf]`. Map the id back to its request.
    let mut rel = rel.to_string();
    let stem = rel
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .split('.')
        .next()
        .unwrap_or_default();
    if !stem.is_empty() {
        let known = app.fileids.join(stem);
        if is_file(&known).await {
            rel = tokio::fs::read_to_string(&known)
                .await
                .with_context(|| format!("reading fileid map {}", known.display()))?;
        }
    }

    let cache_file = app.cache.join(rel.replace('/', "__"));
    if let Some(data) = read_cache(&cache_file).await? {
        return Ok(Some(finish(app, &rel, data).await?));
    }
    if let Some(data) = local_texlive_file(&rel).await? {
        write_cache(&cache_file, &data).await?;
        return Ok(Some(finish(app, &rel, data).await?));
    }
    Ok(None)
}

/// An octet-stream 200 with a Content-Length and an explicit Cache-Control
/// (these routes sit outside the blanket no-cache layer, so every answer
/// states its own freshness), plus any extra headers.
fn octet_stream(data: Vec<u8>, cache_control: &'static str, extra: &[(&'static str, String)]) -> Response {
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CACHE_CONTROL, HeaderValue::from_static(cache_control))
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, data.len().to_string());
    for (name, value) in extra {
        builder = builder.header(HeaderName::from_static(name), value.clone());
    }
    builder
        .body(Body::from(data))
        .expect("a decimal length and static names are always valid headers")
}

/// A failed answer that forbids caching: browsers fresh-cache 404s
/// heuristically, and "no such file" must never outlive the fix that made
/// it true (a package installed, a dump baked by the next image build).
fn no_cache_error(status: StatusCode, message: String) -> Response {
    (
        status,
        [(header::CACHE_CONTROL, HeaderValue::from_static(NO_CACHE))],
        message,
    )
        .into_response()
}

/// Reply 301, the mirror protocol's "file does not exist" marker: the engine
/// worker caches negatives only for 301, so a plain 404 would be re-requested
/// (and re-paid) every time. No-cache as in `no_cache_error` — the 301 must
/// not be remembered past the moment the file appears.
fn proxy_not_found() -> Response {
    Response::builder()
        .status(StatusCode::MOVED_PERMANENTLY)
        .header(header::CACHE_CONTROL, HeaderValue::from_static(NO_CACHE))
        .body(Body::empty())
        .unwrap()
}

/// `Path::is_file` on the blocking pool: a stat on the served tree must not
/// park an executor thread.
async fn is_file(path: &Path) -> bool {
    tokio::fs::metadata(path)
        .await
        .is_ok_and(|meta| meta.is_file())
}

async fn read_cache(path: &Path) -> Result<Option<Vec<u8>>> {
    match tokio::fs::read(path).await {
        Ok(data) => Ok(Some(data)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).context(format!("reading cache {}", path.display())),
    }
}

async fn write_cache(path: &Path, data: &[u8]) -> Result<()> {
    let name = path.file_name().context("cache path has no file name")?;
    let tmp = path.with_file_name(format!(
        "{name}.{}.tmp",
        std::process::id(),
        name = name.to_string_lossy()
    ));
    let parent = path.parent().context("cache path has no parent")?;
    tokio::fs::create_dir_all(parent).await?;
    tokio::fs::write(&tmp, data)
        .await
        .context(format!("writing cache {}", tmp.display()))?;
    tokio::fs::rename(&tmp, path)
        .await
        .context(format!("caching {}", path.display()))?;
    Ok(())
}

/// `<engine>/11/<name>.map` — the requests whose answer `extend_map` below
/// builds from cache file plus texlive-extras/ overrides, and which the
/// TeX Live freshness window therefore skips. dvipdfmx's `/tex/<fileid>`
/// round-trips never carry maps (fonts only), so deciding on the raw URL
/// matches what `extend_map` will see for the resolved request.
fn is_map_request(rel: &str) -> bool {
    let parts: Vec<&str> = rel.split('/').collect();
    parts.len() == 3 && parts[1] == "11" && parts[2].ends_with(".map")
}

/// Map responses get the tracked fallback lines (texlive-extras/<name>)
/// appended. pdfTeX reads the map last-entry-wins, so appends override
/// the cache's stale entries without rewriting the cache itself.
async fn extend_map(app: &App, rel: &str, mut data: Vec<u8>) -> Result<Vec<u8>> {
    if !is_map_request(rel) {
        return Ok(data);
    }
    let extra = app.extras.join(rel.rsplit('/').next().unwrap_or_default());
    if !is_file(&extra).await {
        return Ok(data);
    }
    if !data.is_empty() && !data.ends_with(b"\n") {
        data.push(b'\n');
    }
    let mut extra_bytes = tokio::fs::read(&extra)
        .await
        .context(format!("reading extras {}", extra.display()))?;
    data.append(&mut extra_bytes);
    Ok(data)
}

/// Answer for the worker: extended map plus the stable per-request fileid,
/// recorded so `/tex/<fileid>` round-trips back to this very request.
async fn finish(app: &App, rel: &str, data: Vec<u8>) -> Result<(Vec<u8>, String)> {
    let data = extend_map(app, rel, data).await?;
    let fileid = sha1_hex(rel.as_bytes())[..16].to_string();
    // The mapping only feeds the XeTeX→dvipdfmx /tex/<fileid> round-trip;
    // a failed write must not fail a request whose bytes are already valid.
    let mapping = app.fileids.join(&fileid);
    if !is_file(&mapping).await {
        let write = async {
            tokio::fs::create_dir_all(&app.fileids)
                .await
                .with_context(|| format!("creating fileid map dir {}", app.fileids.display()))?;
            tokio::fs::write(&mapping, rel)
                .await
                .context(format!("writing fileid map {}", mapping.display()))
        };
        if let Err(err) = write.await {
            eprintln!("[texlive] {err:#} — XeTeX font round-trips will miss");
        }
    }
    Ok((data, fileid))
}

/// Resolves `<...>/<basename>` against the local TeX Live with `kpsewhich`;
/// returns the file content or None.
///
/// C-level kpathsea callers (fonts, virtual fonts) request base names without
/// their format's extension, so several extensions are tried and the first
/// hit wins — except PK requests (`pk` format): only a real `.pk` will do
/// (handing pdfTeX a .tfm or .pfb for a PK request is a fatal "bad pk file,
/// expected pre"), and .pk files exist only where mktexpk has run.
async fn local_texlive_file(rel: &str) -> Result<Option<Vec<u8>>> {
    let parts: Vec<&str> = rel.split('/').collect();
    let basename = parts.last().copied().unwrap_or_default();
    if basename.is_empty() || basename.starts_with('.') || !kpsewhich_available() {
        return Ok(None);
    }
    // `<engine>/<kpathsea format id>/<name>`; the engine picks the
    // program-specific search paths (tex/xelatex/... for XeTeX).
    let engine = if parts.len() == 3 { parts[0] } else { "pdftex" };
    let progname = match engine {
        "pdftex" => "pdflatex",
        "xetex" => "xelatex",
        other => other,
    };
    let fmt = if parts.len() >= 3 {
        Some(parts[1])
    } else {
        None
    };

    let mut candidates: Vec<String> = Vec::new();
    if fmt == Some("pk") {
        candidates.push(if basename.ends_with(".pk") {
            basename.to_string()
        } else {
            format!("{basename}.pk")
        });
    } else {
        candidates.push(basename.to_string());
        if !basename.contains('.') {
            let preferred = fmt.and_then(format_ext);
            let exts = [".tfm", ".pfb", ".vf", ".otf", ".ttf", ".enc", ".map"];
            candidates.extend(
                preferred
                    .into_iter()
                    .chain(exts.into_iter().filter(move |ext| Some(*ext) != preferred))
                    .map(|ext| format!("{basename}{ext}")),
            );
        }
    }
    for candidate in &candidates {
        if let Some(data) = lookup_read(engine, progname, &[], candidate).await? {
            return Ok(Some(data));
        }
    }
    // Formats whose search path kpsewhich does not guess from the suffix
    // (e.g. XeTeX's TECkit mappings, `*.tec` in "misc fonts").
    if parts.len() == 3
        && let Some(fmt_name) = fmt.and_then(format_name)
    {
        let extra = format!("-format={fmt_name}");
        if let Some(data) = lookup_read(engine, progname, &[extra], basename).await? {
            return Ok(Some(data));
        }
    }
    Ok(None)
}

/// A kpsewhich hit for one name, read into memory; misses are `Ok(None)`.
async fn lookup_read(
    engine: &str,
    progname: &str,
    extra: &[String],
    name: &str,
) -> Result<Option<Vec<u8>>> {
    let Some(found) = kpsewhich(engine, progname, extra, name).await else {
        return Ok(None);
    };
    Ok(Some(
        tokio::fs::read(&found)
            .await
            .with_context(|| format!("reading {}", found.display()))?,
    ))
}

/// One `kpsewhich` lookup; the first stdout line must be an absolute path
/// to an existing file, or this counts as a miss.
async fn kpsewhich(engine: &str, progname: &str, extra: &[String], name: &str) -> Option<PathBuf> {
    let mut command = tokio::process::Command::new("kpsewhich");
    command
        .arg(format!("-engine={engine}"))
        .arg(format!("-progname={progname}"))
        .args(extra)
        .arg(name)
        .stdin(Stdio::null())
        .kill_on_drop(true);
    // A broken TeX Live must not look like a cache miss: timeout and spawn
    // failure are logged (spawn failure only once, it would repeat per
    // request), while a plain not-found exit stays the normal silent miss.
    let output = match tokio::time::timeout(KPSEWHICH_TIMEOUT, command.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(err)) => {
            static SPAWN_WARNED: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if !SPAWN_WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                eprintln!("[texlive] cannot spawn kpsewhich: {err} — every lookup will miss");
            }
            return None;
        }
        Err(_) => {
            eprintln!(
                "[texlive] kpsewhich timed out after {}s looking up {name}",
                KPSEWHICH_TIMEOUT.as_secs()
            );
            return None;
        }
    };
    let found = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_string();
    let hit = !found.is_empty() && found.starts_with('/') && is_file(Path::new(&found)).await;
    hit.then(|| PathBuf::from(found))
}

async fn tree(State(app): State<Arc<App>>, UrlPath(dir): UrlPath<String>) -> Response {
    match tokio::task::spawn_blocking(move || list_tree(&app, &dir)).await {
        Ok(Ok(body)) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap(),
        Ok(Err((status, message))) => (status, message).into_response(),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

/// GET /tree/<dir> -> {"dir": "<dir>", "files": ["a.tex", "figs/a.png", ...]}
///
/// Paths are relative to <dir> and sorted, so the page can hand a whole
/// submission folder to the same fetch loop it uses for the small samples;
/// a fixture dropped under wasm-test/fixtures/ then needs one line in
/// SAMPLES. Dotfiles are skipped. Local dev server, but the listing stays
/// inside the repository and never follows out of it.
fn list_tree(app: &App, dir: &str) -> Result<String, (StatusCode, &'static str)> {
    let rel = dir.trim_matches('/');
    let target = app
        .root
        .join(rel)
        .canonicalize()
        .or(Err((StatusCode::NOT_FOUND, "no such directory")))?;
    if !target.starts_with(&app.root) || !target.is_dir() {
        return Err((StatusCode::NOT_FOUND, "no such directory"));
    }
    let mut files = Vec::new();
    collect_files(&target, &target, &mut files)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "listing failed"))?;
    files.sort();
    Ok(serde_json::json!({ "dir": rel, "files": files }).to_string())
}

fn collect_files(target: &Path, dir: &Path, files: &mut Vec<String>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with('.'))
        {
            continue; // dotfiles, and whatever hides under them
        }
        if path.is_dir() {
            collect_files(target, &path, files)?;
        } else if path.is_file() {
            files.push(
                path.strip_prefix(target)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Browsers revalidate with `*`, single tags, weak tags and comma lists —
    /// all must answer 304; a foreign tag must not.
    #[test]
    fn if_none_match_accepts_browser_forms() {
        let etag = "\"abc\"";
        let mut headers = HeaderMap::new();
        assert!(!not_modified(&headers, etag));
        for (value, hit) in [
            ("*", true),
            ("\"abc\"", true),
            ("W/\"abc\"", true),
            ("\"def\", \"abc\"", true),
            ("\"def\"", false),
        ] {
            headers.insert(header::IF_NONE_MATCH, HeaderValue::from_static(value));
            assert_eq!(not_modified(&headers, etag), hit, "If-None-Match: {value}");
        }
    }

    /// The TeX Live freshness window keys off this predicate: real map
    /// requests yes, everything else — dvipdfmx's extensionless or
    /// font-extension fileid round-trips, plain package requests, PK — no.
    #[test]
    fn map_request_predicate() {
        assert!(is_map_request("pdftex/11/pdftex.map"));
        assert!(is_map_request("xetex/11/xetex.map"));
        assert!(!is_map_request("xetex/11/0123abcd4567ef89")); // fileid
        assert!(!is_map_request("xetex/11/0123abcd4567ef89.otf")); // round-trip
        assert!(!is_map_request("pdftex/3/cmr10"));
        assert!(!is_map_request("pdftex/pk/600/x"));
        assert!(!is_map_request("pdftex/11/pdftex.map/x"));
    }

    /// The engine's fallback lookups depend on this table staying exactly as
    /// kpse_file_format_type defines it — every id and both accessors.
    #[test]
    fn kpse_format_table() {
        let expected = [
            ("3", Some(".tfm"), None),
            ("6", Some(".bib"), None),
            ("7", Some(".bst"), None),
            ("11", None, Some("map")),
            ("32", Some(".pfb"), Some("type1 fonts")),
            ("33", Some(".vf"), None),
            ("36", Some(".ttf"), Some("truetype fonts")),
            ("41", None, Some("misc fonts")),
            ("44", Some(".enc"), Some("enc files")),
            ("45", None, Some("cmap files")),
            ("46", None, Some("subfont definition files")),
            ("47", Some(".otf"), Some("opentype fonts")),
        ];
        for (id, ext, name) in expected {
            assert_eq!(format_ext(id), ext, "extension for format {id}");
            assert_eq!(format_name(id), name, "format name for {id}");
        }
        // Unknown ids miss on both fallbacks; "pk" is handled before the
        // table and must not appear in it.
        for id in ["pk", "999", "", "0"] {
            assert_eq!(format_ext(id), None, "no extension for {id:?}");
            assert_eq!(format_name(id), None, "no format name for {id:?}");
        }
    }
}
