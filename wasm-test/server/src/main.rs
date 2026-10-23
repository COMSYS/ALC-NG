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
    http::{HeaderName, HeaderValue, StatusCode, header},
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
/// without an extension, mapped to the extension to try first.
fn format_ext(fmt: &str) -> Option<&'static str> {
    Some(match fmt {
        "3" => ".tfm",
        "6" => ".bib",
        "7" => ".bst",
        "32" => ".pfb",
        "33" => ".vf",
        "36" => ".ttf",
        "44" => ".enc",
        "47" => ".otf",
        _ => return None,
    })
}

/// ... and to the format name kpsewhich `-format=` takes, as a last resort.
fn format_name(fmt: &str) -> Option<&'static str> {
    Some(match fmt {
        "11" => "map",
        "32" => "type1 fonts",
        "36" => "truetype fonts",
        "41" => "misc fonts",
        "44" => "enc files",
        "45" => "cmap files",
        "46" => "subfont definition files",
        "47" => "opentype fonts",
        _ => return None,
    })
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

#[tokio::main]
async fn main() -> Result<()> {
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
        root: root.clone(),
        verbose: env::var_os("SERVE_VERBOSE").is_some_and(|v| !v.is_empty()),
    });
    fs::create_dir_all(&app.fmt_cache)
        .with_context(|| format!("creating format cache dir {}", app.fmt_cache.display()))?;

    println!("Serving {} on http://localhost:{port}", root.display());
    println!(
        "texlive proxy: cache -> kpsewhich{} (mirror offline)",
        if kpsewhich_available() {
            ""
        } else {
            " NOT FOUND"
        }
    );

    // Test server: always revalidate, so edits to the page and the patched
    // engine workers take effect after a normal reload.
    let no_cache = SetResponseHeaderLayer::overriding(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache"),
    );
    let static_files = ServiceBuilder::new()
        .layer(no_cache.clone())
        .service(ServeDir::new(&app.root));
    // A bare IP binds directly; anything else (e.g. "localhost") resolves first.
    let addr = format!("{host}:{port}");
    let listener = match addr.parse::<std::net::SocketAddr>() {
        Ok(socket) => tokio::net::TcpListener::bind(socket).await,
        Err(_) => tokio::net::TcpListener::bind((host.as_str(), port)).await,
    }
    .with_context(|| format!("cannot serve on {addr} (SERVE_HOST={host})"))?;
    axum::serve(
        listener,
        Router::new()
            .route(
                "/texlive/{*rel}",
                get(texlive).route_layer(middleware::from_fn_with_state(app.clone(), log_texlive)),
            )
            .route("/tree/{*dir}", get(tree))
            // Prebuilt LaTeX format dumps (read-only): the page fetches one
            // before it considers building the format itself, so a fresh
            // browser skips the multi-minute INITEX run.
            .route("/fmt/{key}/{name}", get(get_fmt))
            // The bench page lives at the site root; the former /wasm-test/
            // location forwards there. The files under wasm-test/ themselves
            // keep being served by the static fallback.
            .route("/", get(index))
            .route("/wasm-test", get(moved_to_root))
            .route("/wasm-test/", get(moved_to_root))
            .layer(no_cache)
            .fallback_service(static_files)
            .with_state(app),
    )
    .await?;
    Ok(())
}

/// The bench page at the site root: read at request time, so edits show up
/// after a plain reload (the whole server answers with no-cache anyway).
async fn index(State(app): State<Arc<App>>) -> Response {
    match fs::read(app.root.join("wasm-test/index.html")) {
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
async fn get_fmt(State(app): State<Arc<App>>, UrlPath((key, name)): UrlPath<(String, String)>) -> Response {
    let Some(path) = fmt_cache_path(&app, &key, &name) else {
        return (StatusCode::NOT_FOUND, "unknown format dump").into_response();
    };
    match fs::read(&path) {
        Ok(data) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .header(header::CONTENT_LENGTH, data.len().to_string())
            .header(HeaderName::from_static("x-fmt-key"), &key)
            .body(Body::from(data))
            .expect("a decimal length is always a valid header"),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            (StatusCode::NOT_FOUND, "no dump cached yet").into_response()
        }
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("reading format dump: {err}"),
        )
            .into_response(),
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

async fn texlive(State(app): State<Arc<App>>, UrlPath(rel): UrlPath<String>) -> Response {
    match serve_texlive(&app, &rel).await {
        Ok(Some((data, fileid))) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .header(header::CONTENT_LENGTH, data.len().to_string())
            // The engine worker derives its /tex cache path from these headers -
            // keep them stable per request so repeated fetches rewrite the same file.
            .header(HeaderName::from_static("fileid"), &fileid)
            .header(HeaderName::from_static("pkid"), &fileid)
            .body(Body::from(data))
            .expect("a hex id and a decimal length are always valid headers"),
        Ok(None) => proxy_not_found(),
        // proxy trouble: report as upstream error
        Err(err) => (
            StatusCode::BAD_GATEWAY,
            format!("texlive proxy failed: {err:#}"),
        )
            .into_response(),
    }
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
        if known.is_file() {
            rel = fs::read_to_string(&known)
                .with_context(|| format!("reading fileid map {}", known.display()))?;
        }
    }

    let cache_file = app.cache.join(rel.replace('/', "__"));
    if let Some(data) = read_cache(&cache_file)? {
        return Ok(Some(finish(app, &rel, data)?));
    }
    if let Some(data) = local_texlive_file(&rel).await? {
        write_cache(&cache_file, &data)?;
        return Ok(Some(finish(app, &rel, data)?));
    }
    Ok(None)
}

/// Reply 301, the mirror protocol's "file does not exist" marker: the engine
/// worker caches negatives only for 301, so a plain 404 would be re-requested
/// (and re-paid) every time.
fn proxy_not_found() -> Response {
    Response::builder()
        .status(StatusCode::MOVED_PERMANENTLY)
        .body(Body::empty())
        .unwrap()
}

fn read_cache(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(data) => Ok(Some(data)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).context(format!("reading cache {}", path.display())),
    }
}

fn write_cache(path: &Path, data: &[u8]) -> Result<()> {
    let name = path.file_name().context("cache path has no file name")?;
    let tmp = path.with_file_name(format!(
        "{name}.{}.tmp",
        std::process::id(),
        name = name.to_string_lossy()
    ));
    fs::create_dir_all(path.parent().context("cache path has no parent")?)?;
    fs::write(&tmp, data).context(format!("writing cache {}", tmp.display()))?;
    fs::rename(&tmp, path).context(format!("caching {}", path.display()))?;
    Ok(())
}

/// Map responses get the tracked fallback lines (texlive-extras/<name>)
/// appended. pdfTeX reads the map last-entry-wins, so appends override
/// the cache's stale entries without rewriting the cache itself.
fn extend_map(app: &App, rel: &str, mut data: Vec<u8>) -> Result<Vec<u8>> {
    let parts: Vec<&str> = rel.split('/').collect();
    if parts.len() != 3 || parts[1] != "11" || !parts[2].ends_with(".map") {
        return Ok(data);
    }
    let extra = app.extras.join(parts[2]);
    if !extra.is_file() {
        return Ok(data);
    }
    if !data.is_empty() && !data.ends_with(b"\n") {
        data.push(b'\n');
    }
    let mut extra_bytes =
        fs::read(&extra).context(format!("reading extras {}", extra.display()))?;
    data.append(&mut extra_bytes);
    Ok(data)
}

/// Answer for the worker: extended map plus the stable per-request fileid,
/// recorded so `/tex/<fileid>` round-trips back to this very request.
fn finish(app: &App, rel: &str, data: Vec<u8>) -> Result<(Vec<u8>, String)> {
    let data = extend_map(app, rel, data)?;
    let digest = Sha1::digest(rel.as_bytes());
    let fileid: String = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()[..16]
        .to_string();
    let mapping = app.fileids.join(&fileid);
    if !mapping.is_file() {
        fs::create_dir_all(&app.fileids)
            .with_context(|| format!("creating fileid map dir {}", app.fileids.display()))?;
        fs::write(&mapping, rel).context(format!("writing fileid map {}", mapping.display()))?;
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
        if let Some(found) = kpsewhich(engine, progname, &[], candidate).await {
            return Ok(Some(
                fs::read(&found).with_context(|| format!("reading {}", found.display()))?,
            ));
        }
    }
    // Formats whose search path kpsewhich does not guess from the suffix
    // (e.g. XeTeX's TECkit mappings, `*.tec` in "misc fonts").
    if parts.len() == 3
        && let Some(fmt_name) = fmt.and_then(format_name)
    {
        let extra = format!("-format={fmt_name}");
        if let Some(found) = kpsewhich(engine, progname, &[extra], basename).await {
            return Ok(Some(
                fs::read(&found).with_context(|| format!("reading {}", found.display()))?,
            ));
        }
    }
    Ok(None)
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
    let output = tokio::time::timeout(KPSEWHICH_TIMEOUT, command.output())
        .await
        .ok()?
        .ok()?;
    let found = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_string();
    (!found.is_empty() && found.starts_with('/') && Path::new(&found).is_file())
        .then(|| PathBuf::from(found))
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
