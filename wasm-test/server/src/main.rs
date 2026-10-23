//! Serves the alc-ng web app (web/, a Vite + React build) together with the
//! repository files it loads, and backs the engine's texlive-on-demand
//! requests with the local TeX Live installation.
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
//! 1. the local TeX Live resolved with `kpsewhich <basename>` for that engine
//!    (PK requests are answered only by real `.pk` files — never by extension
//!    fallbacks); resolved paths are remembered in memory,
//! 2. a 301 — the mirror protocol's "file does not exist" marker, which the
//!    workers cache as a negative. The dead mirror fetch this used to end in
//!    is gone; Dockerfile.wasm runs the server inside the TeX Live full
//!    image, where step 1 answers the entire distribution.
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
//! (The app's build, web/dist, is served at the site root: an overview at
//! /, the app in its two designs at /tool/ and /site/, and the hashed
//! bundles under /assets/. Build it with `pnpm --dir web
//! build`; during development `pnpm --dir web dev` serves the app itself and
//! forwards everything else to this server. The former /wasm-test/ location
//! forwards to /, and all repository files stay served under their paths.)
//!
//! SERVE_ROOT=<dir> serves a different repository root, SERVE_WEB=<dir> a
//! different app build (default: <root>/web/dist), SERVE_HOST=0.0.0.0
//! exposes the server (e.g. inside a container), and SERVE_VERBOSE=1 logs
//! every /texlive/ request with its status.
//!
//! GET /tree/<dir> additionally lists a directory of the repository as JSON,
//! which is how the page fills a sample given as a folder (see SAMPLES in
//! web/shared/lib/samples.ts) without hardcoding every path of a submission.
//!
//! GET /fmt/<key>/<name> serves the prebuilt LaTeX format dumps: the page
//! reads one before it builds the format itself. Dumps are engine-build
//! specific — a native TeX Live .fmt cannot be used — so wasm-test/fmt-cache/
//! is populated at image build time (wasm-test/build-fmt.cjs) and served
//! read-only here. There is deliberately no upload route: a client-written
//! dump would be served to every browser.
//!
//! Read-only: the server never writes to disk, and every route answers GET
//! (and HEAD) only. URLs never reach the file system unchecked: TeX Live
//! requests must have the exact shape the workers send (see
//! `parse_texlive_request`) before a name is handed to kpsewhich, and the
//! static files and /tree/ listings refuse hidden paths (`.git`, `.env`, ...)
//! as well as symlinks that lead out of the served root.
//!
//! Cache policy: everything answers `no-cache` except plain TeX Live
//! packages — those bytes are frozen with the image, so their 200s carry a
//! one-day window and browser caches (and every re-run of a page session)
//! skip re-downloading the package set. Map files and format dumps opt out;
//! see `TEXLIVE_CACHE` and `get_fmt`. The app's /assets/ are named after
//! their content hash and therefore cached for good (`ASSET_CACHE`).

use std::{
    collections::HashMap,
    env, fs, io,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex, OnceLock, PoisonError},
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
use percent_encoding::percent_decode_str;
use sha1::{Digest as _, Sha1};
use tower::ServiceBuilder;
use tower_http::{services::ServeDir, set_header::SetResponseHeaderLayer};

/// `kpsewhich` runs get this long before the attempt counts as a miss —
/// TeX filename lookups on a cold ls-RDB walk the tree, but never that far.
const KPSEWHICH_TIMEOUT: Duration = Duration::from_secs(60);

/// Longest file name the workers may ask for; TeX Live's longest is 90 bytes.
const MAX_NAME_LEN: usize = 100;

/// Upper bound of the in-memory kpsewhich cache. It is emptied when full, so
/// a client sending endless distinct names cannot grow it without bound.
const LOOKUP_CACHE_CAPACITY: usize = 50_000;

/// Marks the fileids handed to the workers (see `TexliveRequest::fileid`).
const FILEID_PREFIX: &str = "tl";

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
    /// The web app's build: index.html for the site root, assets/ for /assets/.
    web: PathBuf,
    /// kpsewhich hits per lookup, so repeated requests skip the process
    /// spawn. Only paths are kept; the bytes are read from TeX Live each time.
    lookups: Mutex<HashMap<String, PathBuf>>,
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

impl App {
    /// The server state for the canonical repository root `root`, serving
    /// the app build in `web`.
    fn new(root: PathBuf, web: PathBuf, verbose: bool) -> Self {
        Self {
            web,
            lookups: Mutex::new(HashMap::new()),
            extras: root.join("wasm-test/texlive-extras"),
            fmt_cache: root.join("wasm-test/fmt-cache"),
            root,
            verbose,
        }
    }
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

    // Not canonicalized: the build may not exist yet (`pnpm dev` serves the
    // app itself), and every request resolves it afresh.
    let web = env::var_os("SERVE_WEB")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("web/dist"));
    let web = if web.is_absolute() {
        web
    } else {
        root.join(web)
    };

    let verbose = env::var_os("SERVE_VERBOSE").is_some_and(|v| !v.is_empty());
    Ok((port, host, Arc::new(App::new(root, web, verbose))))
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

/// The app's bundles carry their content hash in the file name, so a URL
/// never changes its bytes and browsers may keep them indefinitely.
const ASSET_CACHE: &str = "public, max-age=31536000, immutable";

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
        .layer(middleware::from_fn_with_state(app.clone(), guard_static))
        .service(ServeDir::new(&app.root));
    let assets = ServiceBuilder::new()
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static(ASSET_CACHE),
        ))
        .layer(middleware::from_fn_with_state(app.clone(), guard_assets))
        .service(ServeDir::new(app.web.join("assets")));
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
    // The app lives at the site root: an overview there, each design under
    // its own folder. The former /wasm-test/ location forwards to the root;
    // the files under wasm-test/ themselves keep being served by the static
    // fallback.
    let mut pages = Router::new()
        .route("/tree/{*dir}", get(tree))
        .route("/", get(|State(app)| page(app, "index.html")))
        .route("/wasm-test", get(moved_to_root))
        .route("/wasm-test/", get(moved_to_root));
    for design in DESIGNS {
        pages = pages
            .route(
                &format!("/{design}/"),
                get(move |State(app)| page(app, format!("{design}/index.html"))),
            )
            .route(
                &format!("/{design}"),
                get(move || async move { Redirect::temporary(&format!("/{design}/")) }),
            );
    }
    let pages = pages.layer(no_cache);
    Router::new()
        .merge(engine_files)
        .merge(pages)
        .nest_service("/assets", assets)
        .fallback_service(static_files)
        .with_state(app)
}

#[tokio::main]
async fn main() -> Result<()> {
    let (port, host, app) = configure()?;
    println!("Serving {} on http://localhost:{port}", app.root.display());
    if !is_file(&app.web.join("index.html")).await {
        println!(
            "no app build at {} - run `pnpm --dir web build`, or `pnpm --dir web dev` for development",
            app.web.display()
        );
    }
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

/// The app in its two designs, each a page of the build in its own folder
/// (web/tool/, web/site/); the overview at the root links to both.
const DESIGNS: [&str; 2] = ["tool", "site"];

/// A page of the app build (`rel` below it): read at request time, so a new
/// build shows up after a plain reload (it answers no-cache; the bundles it
/// names change their URLs with their content).
async fn page(app: Arc<App>, rel: impl AsRef<Path>) -> Response {
    let page = app.web.join(rel);
    match tokio::fs::read(&page).await {
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
            format!(
                "no app build to serve ({}: {err}) - run `pnpm --dir web build`, or `pnpm --dir web dev` for development",
                page.display()
            ),
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

/// Refuses static requests for hidden paths (`.git`, `.env`, ...) and for
/// files that a symlink leads to outside the served root. `ServeDir` itself
/// already rejects `..` and absolute segments, but follows symlinks and
/// serves dotfiles.
async fn guard_static(State(app): State<Arc<App>>, request: Request, next: Next) -> Response {
    if static_path_allowed(&app.root, request.uri().path()).await {
        next.run(request).await
    } else {
        (StatusCode::NOT_FOUND, "not found").into_response()
    }
}

/// The same guard for the app's /assets/, relative to the build's assets
/// directory (the nested service sees the path without its /assets prefix).
async fn guard_assets(State(app): State<Arc<App>>, request: Request, next: Next) -> Response {
    let allowed = match tokio::fs::canonicalize(app.web.join("assets")).await {
        Ok(root) => static_path_allowed(&root, request.uri().path()).await,
        // no build: ServeDir answers 404
        Err(_) => true,
    };
    if allowed {
        next.run(request).await
    } else {
        (StatusCode::NOT_FOUND, "not found").into_response()
    }
}

/// Whether the (percent-encoded) URL path may be served from `root`, which
/// must be canonical. Decoded the same way `ServeDir` decodes it.
async fn static_path_allowed(root: &Path, uri_path: &str) -> bool {
    let Ok(decoded) = percent_decode_str(uri_path).decode_utf8() else {
        return false;
    };
    let rel = decoded.trim_start_matches('/');
    if !rel_path_allowed(rel) {
        return false;
    }

    let Ok(target) = tokio::fs::canonicalize(root.join(rel)).await else {
        // Missing files get ServeDir's own 404
        return true;
    };
    if !target.starts_with(root) {
        return false;
    }
    // ServeDir answers a directory with its index.html
    if tokio::fs::metadata(&target)
        .await
        .is_ok_and(|meta| meta.is_dir())
    {
        return match tokio::fs::canonicalize(target.join("index.html")).await {
            Ok(index) => index.starts_with(root),
            Err(_) => true,
        };
    }
    true
}

/// A relative URL path without hidden or parent segments (`.git`, `..`) and
/// without characters a path segment must not carry.
fn rel_path_allowed(rel: &str) -> bool {
    !rel.contains(['\\', '\0']) && !rel.split('/').any(|segment| segment.starts_with('.'))
}

/// One request of the engine workers, in the only shapes they send.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TexliveRequest {
    /// `<engine>/<kpathsea format id>/<name>`, engine `pdftex` or `xetex`.
    File {
        engine: &'static str,
        format: u8,
        name: String,
    },
    /// `pdftex/pk/<dpi>/<name>`, a bitmap font at that resolution.
    Pk { dpi: u16, name: String },
}

impl TexliveRequest {
    /// The canonical URL path below /texlive/.
    fn rel(&self) -> String {
        match self {
            Self::File {
                engine,
                format,
                name,
            } => format!("{engine}/{format}/{name}"),
            Self::Pk { dpi, name } => format!("pdftex/pk/{dpi}/{name}"),
        }
    }

    /// The stable id the workers store the file under (`/tex/<fileid>`).
    ///
    /// XeTeX writes that path of every OpenType/TrueType font it loaded into
    /// the XDV; dvipdfmx strips `/tex/` and asks for
    /// `xetex/<format>/<fileid>[.otf|.ttf]`. The id therefore encodes the
    /// request itself (hex, so it has no `.` or `/`), which lets the server
    /// map it back without storing anything.
    fn fileid(&self) -> String {
        let hex: String = self
            .rel()
            .bytes()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        format!("{FILEID_PREFIX}{hex}")
    }

    /// The requested file name.
    fn name(&self) -> &str {
        match self {
            Self::File { name, .. } | Self::Pk { name, .. } => name,
        }
    }

    /// `<engine>/11/<name>.map` — the requests whose answer `extend_map`
    /// builds from TeX Live plus texlive-extras/ overrides, and which the
    /// TeX Live freshness window therefore skips.
    fn is_map(&self) -> bool {
        matches!(self, Self::File { format: 11, name, .. } if name.ends_with(".map"))
    }
}

/// Parses `rel` into a request of the workers, `None` for anything else.
///
/// The name ends up as a kpsewhich argument, so it must be a plain TeX Live
/// file name: kpathsea expands `$VAR` and `~` in names (`$SHELL` resolves to
/// the shell binary) and reads a leading `-` as an option.
fn parse_texlive_request(rel: &str) -> Option<TexliveRequest> {
    let request = parse_texlive_shape(rel)?;

    // dvipdfmx asks for fonts by the fileid XeTeX recorded
    if let TexliveRequest::File { name, .. } = &request
        && let Some(original) = decode_fileid(name)
    {
        return parse_texlive_shape(&original).filter(|original| valid_name(original.name()));
    }

    valid_name(request.name()).then_some(request)
}

/// The shape of a request; its name is checked by the caller.
fn parse_texlive_shape(rel: &str) -> Option<TexliveRequest> {
    match rel.split('/').collect::<Vec<_>>().as_slice() {
        ["pdftex", "pk", dpi, name] => Some(TexliveRequest::Pk {
            dpi: parse_number(dpi)?,
            name: (*name).to_string(),
        }),
        [engine, format, name] => Some(TexliveRequest::File {
            engine: match *engine {
                "pdftex" => "pdftex",
                "xetex" => "xetex",
                _ => return None,
            },
            format: parse_number(format)?,
            name: (*name).to_string(),
        }),
        _ => None,
    }
}

/// A plain decimal number, without sign or surrounding characters.
fn parse_number<T: std::str::FromStr>(text: &str) -> Option<T> {
    (!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

/// A TeX Live file name: `[A-Za-z0-9._+-]`, not starting with `.` or `-`.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_LEN
        && !name.starts_with(['.', '-'])
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'-'))
}

/// The request a fileid (optionally with an extension) was made for.
fn decode_fileid(name: &str) -> Option<String> {
    let stem = name.split('.').next()?;
    let hex = stem.strip_prefix(FILEID_PREFIX)?;
    if hex.is_empty() || hex.len() % 2 != 0 || hex.len() > 2 * (MAX_NAME_LEN + 16) {
        return None;
    }
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
        .collect::<Option<Vec<u8>>>()?;
    String::from_utf8(bytes).ok()
}

async fn texlive(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    UrlPath(rel): UrlPath<String>,
) -> Response {
    let Some(request) = parse_texlive_request(&rel) else {
        return proxy_not_found();
    };
    match serve_texlive(&app, &request).await {
        // The engine worker derives its /tex cache path from the fileid/pkid
        // headers — keep them stable per request so repeated fetches rewrite
        // the same file.
        Ok(Some(data)) => {
            let fileid = request.fileid();
            // Maps stay out of the freshness window: their bytes mix in the
            // tracked texlive-extras/ overrides, not only the frozen image.
            let cache = if request.is_map() {
                NO_CACHE
            } else {
                TEXLIVE_CACHE
            };
            // A content ETag makes even the no-cache maps cheap: the browser
            // revalidates with a ~30-byte request instead of re-downloading
            // the whole file on every compile, while an edited extras override
            // still shows up on the very next load.
            let etag = format!("\"{}\"", sha1_hex(&data));
            if not_modified(&headers, &etag) {
                // The ids go along, so a browser that stored a response with
                // an older id scheme updates it on revalidation.
                return (
                    StatusCode::NOT_MODIFIED,
                    [
                        (header::ETAG, etag),
                        (header::CACHE_CONTROL, cache.to_string()),
                        (HeaderName::from_static("fileid"), fileid.clone()),
                        (HeaderName::from_static("pkid"), fileid),
                    ],
                )
                    .into_response();
            }
            octet_stream(
                data,
                cache,
                &[("etag", etag), ("fileid", fileid.clone()), ("pkid", fileid)],
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

/// `sha1` hex digest, used for the ETags.
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
async fn serve_texlive(app: &App, request: &TexliveRequest) -> Result<Option<Vec<u8>>> {
    match local_texlive_file(app, request).await? {
        Some(data) => Ok(Some(extend_map(app, request, data).await?)),
        None => Ok(None),
    }
}

/// An octet-stream 200 with a Content-Length and an explicit Cache-Control
/// (these routes sit outside the blanket no-cache layer, so every answer
/// states its own freshness), plus any extra headers.
fn octet_stream(
    data: Vec<u8>,
    cache_control: &'static str,
    extra: &[(&'static str, String)],
) -> Response {
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CACHE_CONTROL,
            HeaderValue::from_static(cache_control),
        )
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

/// Map responses get the tracked fallback lines (texlive-extras/<name>)
/// appended. pdfTeX reads the map last-entry-wins, so appends override
/// the distribution's stale entries without touching it.
async fn extend_map(app: &App, request: &TexliveRequest, mut data: Vec<u8>) -> Result<Vec<u8>> {
    if !request.is_map() {
        return Ok(data);
    }
    // A valid name has no `/` and does not start with `.`
    let extra = app.extras.join(request.name());
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

/// Resolves a request against the local TeX Live with `kpsewhich`;
/// returns the file content or None.
///
/// C-level kpathsea callers (fonts, virtual fonts) request base names without
/// their format's extension, so several extensions are tried and the first
/// hit wins — except PK requests: only a real `.pk` at the requested
/// resolution will do (handing pdfTeX a .tfm or .pfb for a PK request is a
/// fatal "bad pk file, expected pre"), and .pk files exist only where
/// mktexpk has run.
async fn local_texlive_file(app: &App, request: &TexliveRequest) -> Result<Option<Vec<u8>>> {
    if !kpsewhich_available() {
        return Ok(None);
    }

    let (engine, format, basename) = match request {
        TexliveRequest::File {
            engine,
            format,
            name,
        } => (*engine, Some(format.to_string()), name.as_str()),
        TexliveRequest::Pk { dpi, name } => {
            let candidate = if name.ends_with(".pk") {
                name.clone()
            } else {
                format!("{name}.pk")
            };
            let dpi = [format!("-dpi={dpi}")];
            return lookup_read(app, "pdftex", "pdflatex", &dpi, &candidate).await;
        }
    };
    // The engine picks the program-specific search paths (tex/xelatex/...
    // for XeTeX).
    let progname = match engine {
        "xetex" => "xelatex",
        _ => "pdflatex",
    };
    let format = format.as_deref();

    let mut candidates = vec![basename.to_string()];
    if !basename.contains('.') {
        let preferred = format.and_then(format_ext);
        let exts = [".tfm", ".pfb", ".vf", ".otf", ".ttf", ".enc", ".map"];
        candidates.extend(
            preferred
                .into_iter()
                .chain(exts.into_iter().filter(move |ext| Some(*ext) != preferred))
                .map(|ext| format!("{basename}{ext}")),
        );
    }
    for candidate in &candidates {
        if let Some(data) = lookup_read(app, engine, progname, &[], candidate).await? {
            return Ok(Some(data));
        }
    }
    // Formats whose search path kpsewhich does not guess from the suffix
    // (e.g. XeTeX's TECkit mappings, `*.tec` in "misc fonts").
    if let Some(fmt_name) = format.and_then(format_name) {
        let extra = [format!("-format={fmt_name}")];
        if let Some(data) = lookup_read(app, engine, progname, &extra, basename).await? {
            return Ok(Some(data));
        }
    }
    Ok(None)
}

/// A kpsewhich hit for one name, read into memory; misses are `Ok(None)`.
/// Hits are remembered in `App::lookups`, misses are not.
async fn lookup_read(
    app: &App,
    engine: &str,
    progname: &str,
    extra: &[String],
    name: &str,
) -> Result<Option<Vec<u8>>> {
    let key = format!("{engine} {progname} {} {name}", extra.join(" "));
    let cached = app
        .lookups
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&key)
        .cloned();
    let found = match cached {
        Some(found) => found,
        None => {
            let Some(found) = kpsewhich(engine, progname, extra, name).await else {
                return Ok(None);
            };
            let mut lookups = app.lookups.lock().unwrap_or_else(PoisonError::into_inner);
            if lookups.len() >= LOOKUP_CACHE_CAPACITY {
                lookups.clear();
            }
            lookups.insert(key, found.clone());
            found
        }
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
        // Never generate missing fonts or files: mktex scripts would write
        // into TEXMFVAR on behalf of whoever sends a request.
        .args([
            "-no-mktex=pk",
            "-no-mktex=mf",
            "-no-mktex=tex",
            "-no-mktex=tfm",
        ])
        .args(extra)
        // The name is validated already; `--` keeps it from ever being an option
        .arg("--")
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
/// SAMPLES. Hidden files and symlinks are skipped. Local dev server, but the
/// listing stays inside the repository and never follows out of it.
fn list_tree(app: &App, dir: &str) -> Result<String, (StatusCode, &'static str)> {
    let rel = dir.trim_matches('/');
    if !rel_path_allowed(rel) {
        return Err((StatusCode::NOT_FOUND, "no such directory"));
    }
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
        let entry = entry?;
        let path = entry.path();
        if path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with('.'))
        {
            continue; // dotfiles, and whatever hides under them
        }
        // The entry's own type: symlinks are neither followed nor listed, so
        // the listing cannot leave the repository or loop.
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_files(target, &path, files)?;
        } else if file_type.is_file() {
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

    fn is_map(rel: &str) -> bool {
        parse_texlive_request(rel).is_some_and(|request| request.is_map())
    }

    /// The TeX Live freshness window keys off this predicate: real map
    /// requests yes, everything else — plain package requests, PK, fonts
    /// asked for by fileid — no.
    #[test]
    fn map_request_predicate() {
        assert!(is_map("pdftex/11/pdftex.map"));
        assert!(is_map("xetex/11/xetex.map"));
        assert!(!is_map("pdftex/3/cmr10"));
        assert!(!is_map("pdftex/pk/600/x"));
        assert!(!is_map("pdftex/11/pdftex.map/x"));
        let font = parse_texlive_request("xetex/47/texgyreheros-regular.otf").unwrap();
        assert!(!is_map(&format!("xetex/47/{}.otf", font.fileid())));
    }

    /// Only the shapes the workers send reach kpsewhich, with plain names.
    #[test]
    fn texlive_requests_are_parsed_strictly() {
        assert_eq!(
            parse_texlive_request("pdftex/26/article.cls"),
            Some(TexliveRequest::File {
                engine: "pdftex",
                format: 26,
                name: "article.cls".into()
            })
        );
        assert_eq!(
            parse_texlive_request("pdftex/pk/657/cmr10"),
            Some(TexliveRequest::Pk {
                dpi: 657,
                name: "cmr10".into()
            })
        );
        assert!(parse_texlive_request("xetex/47/lmroman10-regular.otf").is_some());
        assert!(parse_texlive_request("pdftex/3/ec-lmr10").is_some());

        for rel in [
            // kpathsea expansion and option injection
            "pdftex/3/$SHELL",
            "pdftex/3/${HOME}",
            "pdftex/3/~",
            "pdftex/3/-var-value=SHELL",
            "pdftex/3/--help",
            // hidden files and traversal
            "pdftex/3/.bashrc",
            "pdftex/3/..",
            "pdftex/3/../../etc/passwd",
            "pdftex/3/a\\b",
            // shapes the workers never send
            "pdftex__33/ptmr7t",
            "ptmr7t",
            "luatex/3/cmr10",
            "pdftex/abc/cmr10",
            "pdftex/+3/cmr10",
            "pdftex/999/cmr10",
            "pdftex/3/",
            "pdftex/3/cmr10/x",
            "pdftex/pk/abc/cmr10",
            "pdftex/pk/99999999/cmr10",
            "xetex/pk/600/cmr10",
            "pdftex/3/my file",
        ] {
            assert_eq!(parse_texlive_request(rel), None, "{rel:?}");
        }
        assert_eq!(
            parse_texlive_request(&format!("pdftex/3/{}", "a".repeat(MAX_NAME_LEN + 1))),
            None
        );
    }

    /// dvipdfmx's `<fileid>[.ext]` requests map back to the original request
    /// without any stored state, and a fileid cannot smuggle in a name the
    /// direct route would refuse.
    #[test]
    fn fileids_round_trip() {
        let font = parse_texlive_request("xetex/47/texgyreheros-regular.otf").unwrap();
        let fileid = font.fileid();
        assert!(!fileid.contains(['.', '/']));
        assert_eq!(
            parse_texlive_request(&format!("xetex/47/{fileid}")),
            Some(font.clone())
        );
        assert_eq!(
            parse_texlive_request(&format!("xetex/36/{fileid}.otf")),
            Some(font)
        );

        let encode = |rel: &str| {
            let hex: String = rel.bytes().map(|b| format!("{b:02x}")).collect();
            format!("xetex/47/{FILEID_PREFIX}{hex}")
        };
        assert_eq!(parse_texlive_request(&encode("pdftex/3/$SHELL")), None);
        assert_eq!(parse_texlive_request(&encode("../../etc/passwd")), None);
        // Decoded once only: a nested fileid stays a plain (missing) name
        let nested = parse_texlive_request("xetex/47/x.otf").unwrap().fileid();
        assert_eq!(
            parse_texlive_request(&encode(&format!("xetex/47/{nested}"))),
            Some(TexliveRequest::File {
                engine: "xetex",
                format: 47,
                name: nested
            })
        );
    }

    /// A scratch directory below the system temp dir, removed on drop.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let dir = env::temp_dir().join(format!("wasm-serve-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir.canonicalize().unwrap())
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A served root with a page, a sample, hidden files and symlinks that
    /// lead out of it; `outside` holds the files those symlinks point at.
    fn served_root(tag: &str) -> (Scratch, Scratch) {
        let root = Scratch::new(&format!("{tag}-root"));
        let outside = Scratch::new(&format!("{tag}-outside"));
        fs::write(outside.0.join("secret.txt"), "secret").unwrap();
        fs::create_dir_all(root.0.join("wasm-test/fmt-cache")).unwrap();
        fs::create_dir_all(root.0.join("web/dist/assets")).unwrap();
        fs::write(root.0.join("web/dist/index.html"), "<html></html>").unwrap();
        for design in DESIGNS {
            fs::create_dir_all(root.0.join(format!("web/dist/{design}"))).unwrap();
            fs::write(root.0.join(format!("web/dist/{design}/index.html")), design).unwrap();
        }
        fs::write(root.0.join("web/dist/assets/index-abc123.js"), "app").unwrap();
        fs::write(root.0.join("web/dist/assets/.hidden"), "hidden").unwrap();
        fs::create_dir_all(root.0.join("sample/figs")).unwrap();
        fs::write(root.0.join("sample/main.tex"), "main").unwrap();
        fs::write(root.0.join("sample/figs/a.png"), "png").unwrap();
        fs::create_dir_all(root.0.join(".git")).unwrap();
        fs::write(root.0.join(".git/config"), "config").unwrap();
        fs::write(root.0.join("sample/.env"), "env").unwrap();
        std::os::unix::fs::symlink(outside.0.join("secret.txt"), root.0.join("sample/leak.txt"))
            .unwrap();
        std::os::unix::fs::symlink(&outside.0, root.0.join("sample/out")).unwrap();
        (root, outside)
    }

    /// Every file below `dir` with its contents, to prove nothing changed.
    fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        let mut entries = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(&dir).unwrap() {
                let entry = entry.unwrap();
                let file_type = entry.file_type().unwrap();
                if file_type.is_dir() {
                    stack.push(entry.path());
                }
                let contents = if file_type.is_file() {
                    fs::read(entry.path()).unwrap()
                } else {
                    Vec::new()
                };
                entries.push((entry.path(), contents));
            }
        }
        entries.sort();
        entries
    }

    async fn send(app: &Arc<App>, method: &str, uri: &str) -> StatusCode {
        use tower::ServiceExt as _;

        let request = Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .unwrap();
        build_router(app.clone())
            .oneshot(request)
            .await
            .unwrap()
            .status()
    }

    /// Static files: hidden paths and symlinks out of the root are refused
    /// in every encoding, regular files are served.
    #[tokio::test]
    async fn static_files_stay_inside_the_root() {
        let (root, _outside) = served_root("static");
        let app = Arc::new(App::new(root.0.clone(), root.0.join("web/dist"), false));

        assert_eq!(send(&app, "GET", "/sample/main.tex").await, StatusCode::OK);
        assert_eq!(
            send(&app, "GET", "/sample/figs/a.png").await,
            StatusCode::OK
        );
        assert_eq!(send(&app, "GET", "/").await, StatusCode::OK);
        for uri in [
            "/.git/config",
            "/%2egit/config",
            "/sample/.env",
            "/sample/%2Eenv",
            "/sample/leak.txt",
            "/sample/out/secret.txt",
            "/sample/out/",
            "/../secret.txt",
            "/sample/%2e%2e/%2e%2e/secret.txt",
            "/sample/..%2f..%2fsecret.txt",
            "/sample%5c..%5c.git%5cconfig",
            "/wasm-test/fmt-cache/../../.git/config",
        ] {
            assert_eq!(
                send(&app, "GET", uri).await,
                StatusCode::NOT_FOUND,
                "GET {uri}"
            );
        }
    }

    /// /tree/ lists regular files only: no hidden files, no symlinks, and no
    /// directory outside the root or below a hidden one.
    #[test]
    fn tree_listing_stays_inside_the_root() {
        let (root, _outside) = served_root("tree");
        let app = App::new(root.0.clone(), root.0.join("web/dist"), false);

        let listing: serde_json::Value =
            serde_json::from_str(&list_tree(&app, "sample").unwrap()).unwrap();
        assert_eq!(
            listing["files"],
            serde_json::json!(["figs/a.png", "main.tex"])
        );

        for dir in [
            ".git",
            "sample/../.git",
            "..",
            "../..",
            "sample/out",
            "/etc",
        ] {
            assert!(list_tree(&app, dir).is_err(), "/tree/{dir}");
        }
    }

    async fn get_response(app: &Arc<App>, uri: &str) -> Response {
        use tower::ServiceExt as _;

        let request = Request::builder().uri(uri).body(Body::empty()).unwrap();
        build_router(app.clone()).oneshot(request).await.unwrap()
    }

    /// The app build: the overview at the root, a page per design below it,
    /// hashed bundles under /assets/ with a permanent cache, and nothing
    /// hidden or outside the build.
    #[tokio::test]
    async fn app_build_is_served() {
        let (root, _outside) = served_root("app");
        let app = Arc::new(App::new(root.0.clone(), root.0.join("web/dist"), false));

        let index = get_response(&app, "/").await;
        assert_eq!(index.status(), StatusCode::OK);
        assert_eq!(index.headers()[header::CACHE_CONTROL], NO_CACHE);

        for design in DESIGNS {
            let page = get_response(&app, &format!("/{design}/")).await;
            assert_eq!(page.status(), StatusCode::OK);
            assert_eq!(page.headers()[header::CACHE_CONTROL], NO_CACHE);
            let body = axum::body::to_bytes(page.into_body(), 1024).await.unwrap();
            assert_eq!(body, design.as_bytes());

            let bare = get_response(&app, &format!("/{design}")).await;
            assert_eq!(bare.status(), StatusCode::TEMPORARY_REDIRECT);
            assert_eq!(bare.headers()[header::LOCATION], format!("/{design}/"));
        }

        let bundle = get_response(&app, "/assets/index-abc123.js").await;
        assert_eq!(bundle.status(), StatusCode::OK);
        assert_eq!(bundle.headers()[header::CACHE_CONTROL], ASSET_CACHE);

        for uri in [
            "/assets/.hidden",
            "/assets/%2ehidden",
            "/assets/../index.html",
            "/assets/%2e%2e/%2e%2e/%2e%2e/.git/config",
            "/assets/missing.js",
        ] {
            assert_eq!(
                get_response(&app, uri).await.status(),
                StatusCode::NOT_FOUND,
                "GET {uri}"
            );
        }

        // without a build, the root explains how to make one
        let bare = Arc::new(App::new(root.0.clone(), root.0.join("web/missing"), false));
        assert_eq!(
            get_response(&bare, "/").await.status(),
            StatusCode::NOT_FOUND
        );
    }

    /// No route accepts a writing method, and serving requests leaves the
    /// served tree byte for byte as it was.
    #[tokio::test]
    async fn server_has_no_write_path() {
        let (root, outside) = served_root("readonly");
        let app = Arc::new(App::new(root.0.clone(), root.0.join("web/dist"), false));
        let before = (snapshot(&root.0), snapshot(&outside.0));

        let uris = [
            "/",
            "/tool/",
            "/site/",
            "/assets/index-abc123.js",
            "/wasm-test/",
            "/sample/main.tex",
            "/sample/new.tex",
            "/tree/sample",
            "/texlive/pdftex/26/article.cls",
            "/texlive/pdftex/3/cmr10",
            "/fmt/key/swiftlatexpdftex.fmt",
        ];
        for uri in uris {
            for method in ["POST", "PUT", "PATCH", "DELETE"] {
                assert_eq!(
                    send(&app, method, uri).await,
                    StatusCode::METHOD_NOT_ALLOWED,
                    "{method} {uri}"
                );
            }
        }
        for uri in uris {
            send(&app, "GET", uri).await;
        }

        assert_eq!((snapshot(&root.0), snapshot(&outside.0)), before);
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
