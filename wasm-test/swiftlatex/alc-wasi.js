/* alc-ng patch: WASI preview1 shim to run the alc_ng wasm cdylib inside this
 * worker. The module (target wasm32-wasip1) gets its filesystem from the
 * engine's Emscripten MEMFS, so a submission staged for compilation can be
 * cleaned by the real Rust pipeline without copying anything.
 *
 * Commands handled (see wasm-test/swiftlatex/README.md):
 *   alcng_load {wasm}        instantiate alc_ng.wasm (ArrayBuffer)
 *   alcng_run  {json}        call alc_ng_run with the pipeline config JSON
 *   alcng_list {path}        recursively list {path,size} entries under a path
 *   alcng_read {path}        file contents as Uint8Array
 *   alcng_rm   {path}        remove a file or directory tree
 *
 * Notes on the WASI surface:
 * - Preopens: fd 3 = "/", fd 4 = "." (Emscripten's cwd). wasi-libc resolves
 *   all paths through these.
 * - This Emscripten build's FS throws its own errno dialect (observed:
 *   44 = ENOENT, 20 = EEXIST, 54 = ENOTDIR), so existence and type checks
 *   are done up front via FS.analyzePath and only unexpected errors go
 *   through the small translation map.
 * - Timestamp setting is a no-op (MEMFS times are cosmetic; the pipeline's
 *   timestamp scrambling would not be observable anyway).
 */
var AlcWasi = (() => {
  "use strict";

  let FS = null;        // set by dispatch() from the engine worker scope
  let inst = null;      // WebAssembly.Instance of alc_ng.wasm
  let ctorsRan = false;

  const te = new TextDecoder("utf-8", { fatal: false });

  // WASI-visible file descriptors. 0/1/2 are stdio, 3/4 the preopens.
  const PREOPEN_ROOT = 3;
  const PREOPEN_CWD = 4;
  const fds = new Map(); // fd -> {stream, path, dir, append}
  let nextFd = 100;
  const stdioPending = {};  // fd -> partial line not yet terminated by "\n"

  // preview1 errno values (alphabetical wasi/api.h numbering — NB: these are
  // NOT the Linux numbers; e.g. EINVAL is 28, ENOENT is 44).
  const E = {
    SUCCESS: 0, EAGAIN: 6, EACCES: 2, EBADF: 8, EBUSY: 10, EEXIST: 20,
    EINTR: 27, EINVAL: 28, EIO: 29, EISDIR: 31, ELOOP: 32, EMFILE: 33,
    ENAMETOOLONG: 37, ENFILE: 41, ENODEV: 43, ENOENT: 44, ENOLCK: 46,
    ENOMEM: 48, ENOSPC: 51, ENOSYS: 52, ENOTDIR: 54, ENOTEMPTY: 55,
    ENOTSUP: 58, EOVERFLOW: 61, EPERM: 63, EPIPE: 64, ERANGE: 68,
    EROFS: 69, ESPIPE: 70, ETIMEDOUT: 73, EXDEV: 75, EWOULDBLOCK: 76,
  };

  // __wasi_dirent_t header (24 bytes): d_next u64 @0, d_ino u64 @8,
  // d_namlen u32 @16, d_type u8 @20; the name follows unpadded at @24.
  const DIRENT_SIZE = 24;
  const DIRENT_TYPE_AT = 20;
  const DIRENT_NAME_AT = 24;

  // --- wasm memory helpers -------------------------------------------------
  function memory() { return inst.exports.memory; }
  function u8arr(ptr, len) { return new Uint8Array(memory().buffer, ptr, len); }
  function dv() { return new DataView(memory().buffer); }
  function rU32(p) { return dv().getUint32(p, true); }
  function wU8(p, v) { dv().setUint8(p, v); }
  function wU16(p, v) { dv().setUint16(p, v, true); }
  function wU32(p, v) { dv().setUint32(p, v >>> 0, true); }
  function wU64(p, v) { dv().setBigUint64(p, BigInt(v), true); }
  function readStr(ptr, len) { return te.decode(u8arr(ptr, len)); }
  function readCStr(ptr) {
    const buf = new Uint8Array(memory().buffer);
    let end = ptr;
    while (end < buf.length && buf[end] !== 0) end++;
    return te.decode(buf.subarray(ptr, end));
  }

  // --- path handling ---------------------------------------------------------
  function normalizePath(p) {
    const out = [];
    for (const seg of p.split("/")) {
      if (!seg || seg === ".") continue;
      if (seg === "..") out.pop();  // popping past the root is a no-op
      else out.push(seg);
    }
    return "/" + out.join("/");
  }

  // Emscripten builds differ: FS.cwd is a plain string in older ones and an
  // accessor function in newer ones (with the string in FS.currentPath).
  function fsCwd() {
    return typeof FS.cwd === "function" ? FS.cwd() : FS.cwd;
  }

  // FS.analyzePath reports existence only (no isDirectory in this build);
  // add the directory flag every caller here needs.
  function analyze(path, dontFollow) {
    const info = FS.analyzePath(path, dontFollow);
    info.isDirectory = !!(info.exists && info.object && FS.isDir(info.object.mode));
    return info;
  }

  function fdDir(fd) {
    if (fd === PREOPEN_ROOT) return "/";
    if (fd === PREOPEN_CWD || fd === -100 /* AT_FDCWD */) return fsCwd();
    const entry = fds.get(fd);
    return entry ? entry.dir : undefined;
  }

  function resolveAt(dirfd, pathStr) {
    if (!pathStr) return null;
    const base = fdDir(dirfd);
    if (base === undefined) return null;
    return normalizePath(pathStr.startsWith("/") ? pathStr : base + "/" + pathStr);
  }

  // wasi_filetype values (preview1): 2 chrdev, 3 directory, 4 regular,
  // 7 symbolic link. Directories must NOT report 7 — that is the symlink
  // type, and std's canonicalize/realpath follows reported symlinks.
  function nodeType(node) {
    if (FS.isDir(node.mode)) return 3;
    if (FS.isLink && FS.isLink(node.mode)) return 7;
    if (FS.isFile && FS.isFile(node.mode)) return 4;
    if (FS.isChrdev && FS.isChrdev(node.mode)) return 2;
    return 0;
  }

  // MEMFS nodes carry no `length`; the size lives behind getattr (usedBytes
  // for files, a nominal 4096 for directories).
  function nodeSize(node) {
    try { return node.node_ops.getattr(node).size; }
    catch (e) { return node.usedBytes ?? 0; }
  }

  function fillFilestat(node, outPtr) {
    wU64(outPtr, 1n);                                     // dev
    wU64(outPtr + 8, BigInt(node.id ?? 1));               // ino
    wU8(outPtr + 16, nodeType(node));                     // filetype
    wU64(outPtr + 24, 1n);                                // nlink
    wU64(outPtr + 32, BigInt(nodeSize(node)));          // size
    const ts = BigInt(node.timestamp ? +new Date(node.timestamp) : Date.now()) * 1000000n;
    wU64(outPtr + 40, ts); wU64(outPtr + 48, ts); wU64(outPtr + 56, ts);
  }

  // Translate unexpected FS errors (this build's errno dialect) to preview1.
  function fsCatch(e, ctx) {
    console.warn(`[alc-wasi] FS error in ${ctx}: errno ${e && e.errno}`);
    const observed = { 44: E.ENOENT, 20: E.EEXIST, 54: E.ENOTDIR };
    return observed[e && e.errno] ?? E.EINVAL;
  }

  // --- WASI imports ---------------------------------------------------------
  const wasiFns = {
    // stdio goes to the worker console, which forwards to the page log.
    fd_write(fd, iovsPtr, iovsLen, nOut) {
      let total = 0;
      const parts = [];
      for (let i = 0; i < iovsLen; i++) {
        const base = rU32(iovsPtr + i * 8), len = rU32(iovsPtr + i * 8 + 4);
        if (len) parts.push(u8arr(base, len));
        total += len;
      }
      if (fd <= 2) {
        // Line-buffer stdio: the log crate emits one record in several writes.
        let text = stdioPending[fd] ?? "";
        for (const p of parts) text += te.decode(p);
        const lines = text.split("\n");
        stdioPending[fd] = lines.pop();
        for (const line of lines) {
          if (line) (fd === 2 ? console.error : console.log).call(console, line);
        }
        wU32(nOut, total);
        return E.SUCCESS;
      }
      const entry = fds.get(fd);
      if (!entry || !entry.stream) return E.EBADF;
      try {
        const all = new Uint8Array(total);
        let off = 0;
        for (const p of parts) { all.set(p, off); off += p.length; }
        const pos = entry.append ? FS.llseek(entry.stream, 0, 2) : entry.stream.position;
        const n = FS.write(entry.stream, all, 0, total, pos);
        entry.stream.position = pos + n;
        wU32(nOut, n);
        return E.SUCCESS;
      } catch (e) { return fsCatch(e, "fd_write"); }
    },

    fd_read(fd, iovsPtr, iovsLen, nOut) {
      if (fd <= 2) { wU32(nOut, 0); return E.SUCCESS; }  // EOF
      const entry = fds.get(fd);
      if (!entry) return E.EBADF;
      if (!entry.stream) return E.EBADF;
      try {
        let total = 0;
        for (let i = 0; i < iovsLen; i++) {
          const base = rU32(iovsPtr + i * 8), len = rU32(iovsPtr + i * 8 + 4);
          if (!len) continue;
          const n = FS.read(entry.stream, u8arr(base, len), 0, len, entry.stream.position);
          entry.stream.position += n;
          total += n;
          if (n < len) break;
        }
        wU32(nOut, total);
        return E.SUCCESS;
      } catch (e) { return fsCatch(e, "fd_read"); }
    },

    fd_seek(fd, delta, whence, newOffOut) {
      const entry = fds.get(fd);
      if (!entry || !entry.stream) return E.EBADF;
      try {
        const pos = FS.llseek(entry.stream, Number(delta), whence);
        wU64(newOffOut, BigInt(pos));
        return E.SUCCESS;
      } catch (e) { return fsCatch(e, "fd_seek"); }
    },

    fd_close(fd) {
      if (fd <= 4) return E.SUCCESS;   // stdio and the preopens stay open
      const entry = fds.get(fd);
      if (!entry) return E.EBADF;
      fds.delete(fd);
      if (entry.stream) { try { FS.close(entry.stream); } catch (e) {} }
      return E.SUCCESS;
    },

    fd_fdstat_get(fd, out) {
      let filetype = 2, flags = 0;  // stdio: character device (is_terminal)
      if (fd > 2) {
        const entry = fds.get(fd);
        if (!entry) return E.EBADF;
        flags = entry.append ? 1 : 0;  // __WASI_FDFLAG_APPEND
        filetype = entry.dir ? 3 : entry.stream ? nodeType(entry.stream.node) : 0;
      }
      // __wasi_fdstat_t (24 bytes): filetype u8 @0, flags u16 @2,
      // rights_base u64 @8, rights_inheriting u64 @16. wasi-libc masks the
      // rights of every openat() with the dirfd's inheriting rights.
      wU8(out, filetype);
      wU8(out + 1, 0);
      wU16(out + 2, flags);
      wU32(out + 4, 0);
      wU64(out + 8, 0xFFFFFFFFFFFFFFFFn);    // rights
      wU64(out + 16, 0xFFFFFFFFFFFFFFFFn);   // rights inheriting
      return E.SUCCESS;
    },

    fd_filestat_get(fd, out) {
      if (fd <= 2) {
        wU64(out, 0n); wU64(out + 8, 0n); wU8(out + 16, 2);
        wU64(out + 24, 1n); wU64(out + 32, 0n);
        const now = BigInt(Date.now()) * 1000000n;
        wU64(out + 40, now); wU64(out + 48, now); wU64(out + 56, now);
        return E.SUCCESS;
      }
      const entry = fds.get(fd);
      if (!entry || !entry.stream) return E.EBADF;
      fillFilestat(entry.stream.node, out);
      return E.SUCCESS;
    },

    // MEMFS timestamps are cosmetic; the pipeline's scramble is a no-op.
    fd_filestat_set_times() { return E.SUCCESS; },
    path_filestat_set_times() { return E.SUCCESS; },

    path_open(dirfd, lookupFlags, pathPtr, pathLen, oflags, rightsBase, rightsInh, fdflags, openedFdOut) {
      const full = resolveAt(dirfd, readStr(pathPtr, pathLen));
      if (full === null) return E.EBADF;
      const creat = !!(oflags & 1), wantDir = !!(oflags & 2);
      const excl = !!(oflags & 4), trunc = !!(oflags & 8);
      const append = !!(fdflags & 1);
      const canWrite = rightsBase !== null && (rightsBase & 0x40n) !== 0n;
      const canRead = rightsBase === null || (rightsBase & 0x2n) !== 0n;
      let emFlags = canWrite ? (canRead ? 2 : 1) : 0;   // O_WRONLY/O_RDWR/O_RDONLY
      if (creat) emFlags |= 64;     // O_CREAT in this build
      if (excl) emFlags |= 128;     // O_EXCL
      if (trunc) emFlags |= 512;    // O_TRUNC
      if (append) emFlags |= 1024;  // O_APPEND
      let info;
      try { info = analyze(full, !(lookupFlags & 1)); }
      catch (e) { return fsCatch(e, `path_open stat ${full}`); }
      if (excl && creat && info.exists) return E.EEXIST;
      if (!creat && !info.exists) return E.ENOENT;
      if (info.exists && info.isDirectory) {
        // Directories never open for writing; read-only opens (this is how
        // opendir reaches us) get a plain directory handle, never a stream.
        if (canWrite) return E.EISDIR;
        const fd = nextFd++;
        fds.set(fd, { stream: null, path: full, dir: full, append: false });
        wU32(openedFdOut, fd);
        return E.SUCCESS;
      }
      if (wantDir) return E.ENOTDIR;   // existed, was not a directory
      try {
        const stream = FS.open(full, emFlags);
        const fd = nextFd++;
        fds.set(fd, {
          stream, path: full, append,
          dir: FS.isDir(stream.node.mode) ? FS.getPath(stream.node) : null,
        });
        wU32(openedFdOut, fd);
        return E.SUCCESS;
      } catch (e) { return fsCatch(e, `path_open ${full}`); }
    },

    path_filestat_get(dirfd, lookupFlags, pathPtr, pathLen, out) {
      const full = resolveAt(dirfd, readStr(pathPtr, pathLen));
      if (full === null) return E.EBADF;
      const info = analyze(full, !(lookupFlags & 1));
      if (!info.exists) return E.ENOENT;
      fillFilestat(info.object, out);
      return E.SUCCESS;
    },

    path_create_directory(dirfd, pathPtr, pathLen) {
      const full = resolveAt(dirfd, readStr(pathPtr, pathLen));
      if (full === null) return E.EBADF;
      if (analyze(full).exists) return E.EEXIST;
      try { FS.mkdir(full); return E.SUCCESS; }
      catch (e) { return fsCatch(e, `path_create_directory ${full}`); }
    },

    path_remove_directory(dirfd, pathPtr, pathLen) {
      const full = resolveAt(dirfd, readStr(pathPtr, pathLen));
      if (full === null) return E.EBADF;
      const info = analyze(full);
      if (!info.exists) return E.ENOENT;
      if (!info.isDirectory) return E.ENOTDIR;
      try {
        if (FS.readdir(full).length > 2) return E.ENOTEMPTY;  // beyond "." and ".."
        FS.rmdir(full);
        return E.SUCCESS;
      } catch (e) { return fsCatch(e, `path_remove_directory ${full}`); }
    },

    path_unlink_file(dirfd, pathPtr, pathLen) {
      const full = resolveAt(dirfd, readStr(pathPtr, pathLen));
      if (full === null) return E.EBADF;
      const info = analyze(full);
      if (!info.exists) return E.ENOENT;
      if (info.isDirectory) return E.EISDIR;
      try { FS.unlink(full); return E.SUCCESS; }
      catch (e) { return fsCatch(e, `path_unlink_file ${full}`); }
    },

    path_rename(oldFd, oldPtr, oldLen, newFd, newPtr, newLen) {
      const from = resolveAt(oldFd, readStr(oldPtr, oldLen));
      const to = resolveAt(newFd, readStr(newPtr, newLen));
      if (from === null || to === null) return E.EBADF;
      if (!analyze(from).exists) return E.ENOENT;
      try { FS.rename(from, to); return E.SUCCESS; }
      catch (e) { return fsCatch(e, `path_rename ${from} -> ${to}`); }
    },

    path_readlink(dirfd, pathPtr, pathLen, buf, bufLen, bufUsedOut) {
      const full = resolveAt(dirfd, readStr(pathPtr, pathLen));
      if (full === null) return E.EBADF;
      const info = analyze(full, true);
      if (!info.exists) return E.ENOENT;
      if (!FS.isLink || !FS.isLink(info.object.mode)) return E.EINVAL;
      try {
        const target = FS.readlink(full);
        const bytes = new TextEncoder().encode(target);
        const n = Math.min(bytes.length, Math.max(bufLen - 1, 0));
        u8arr(buf, n).set(bytes.subarray(0, n));
        wU32(bufUsedOut, n);
        return E.SUCCESS;
      } catch (e) { return fsCatch(e, `path_readlink ${full}`); }
    },

    fd_readdir(fd, buf, bufLen, cookie, usedOut) {
      const entry = fds.get(fd);
      if (!entry || !entry.dir) return E.EBADF;
      let names;
      try { names = FS.readdir(entry.dir); }
      catch (e) { return fsCatch(e, `fd_readdir ${entry.dir}`); }
      // Serialize dirents (header + unpadded name) from the cookie onwards;
      // the last one may be cut off. A full buffer tells wasi-libc there is
      // more to read, so it calls again with a larger buffer or later cookie.
      const recs = [];
      let total = 0;
      for (let idx = Number(cookie); idx < names.length && total < bufLen; idx++) {
        const name = names[idx];
        const enc = new TextEncoder().encode(name);
        const rec = new Uint8Array(DIRENT_SIZE + enc.length);
        const v = new DataView(rec.buffer);
        v.setBigUint64(0, BigInt(idx + 1), true);   // d_next = resume cookie
        v.setBigUint64(8, 1n, true);                // d_ino (unused by std)
        v.setUint32(16, enc.length, true);          // d_namlen
        const child = (name === "." || name === "..")
          ? 3
          : analyze(entry.dir === "/" ? "/" + name : entry.dir + "/" + name).isDirectory ? 3 : 4;
        v.setUint8(DIRENT_TYPE_AT, child);          // d_type
        rec.set(enc, DIRENT_NAME_AT);
        recs.push(rec);
        total += rec.length;
      }
      let off = 0;
      for (const rec of recs) {
        const n = Math.min(rec.length, bufLen - off);
        if (n <= 0) break;
        u8arr(buf + off, n).set(rec.subarray(0, n));
        off += n;
      }
      wU32(usedOut, off);
      return E.SUCCESS;
    },

    fd_prestat_get(fd, out) {
      const name = fd === PREOPEN_ROOT ? "/" : fd === PREOPEN_CWD ? fsCwd() : null;
      if (name === null) return E.EBADF;
      wU8(out, 0);                 // tag: directory
      wU32(out + 4, name.length);  // name length
      return E.SUCCESS;
    },

    fd_prestat_dir_name(fd, pathPtr, pathLen) {
      // The cwd preopen is reported with its absolute path (not ".") so that
      // every dirfd resolves against an unambiguous absolute base.
      const name = fd === PREOPEN_ROOT ? "/" : fd === PREOPEN_CWD ? fsCwd() : null;
      if (name === null) return E.EBADF;
      if (name.length > pathLen) return E.EINVAL;
      u8arr(pathPtr, name.length).set(new TextEncoder().encode(name));
      return E.SUCCESS;
    },

    // TMPDIR=/tmp keeps the pipeline's TempDir inside the engine FS.
    environ_sizes_get(cntP, sizeP) {
      const env = AlcWasi_ENV;
      let size = 0;
      for (const e of env) size += e.length + 1;
      wU32(cntP, env.length);
      wU32(sizeP, size);
      return E.SUCCESS;
    },

    environ_get(arrP, bufP) {
      let off = 0;
      AlcWasi_ENV.forEach((e, i) => {
        const bytes = new TextEncoder().encode(e);
        wU32(arrP + i * 4, bufP + off);
        const view = u8arr(bufP + off, bytes.length + 1);
        view.set(bytes);
        view[bytes.length] = 0;
        off += bytes.length + 1;
      });
      return E.SUCCESS;
    },

    clock_time_get(id, precision, out) {
      // __wasi_timestamp_t: a single u64 of nanoseconds.
      const ns = id === 0
        ? BigInt(Date.now()) * 1000000n
        : BigInt(Math.round(performance.now() * 1000)) * 1000n;
      wU64(out, ns);
      return E.SUCCESS;
    },

    clock_res_get(_id, out) {
      wU64(out, 1000000n);  // 1 ms, as a u64 timestamp
      return E.SUCCESS;
    },

    random_get(buf, len) {
      const view = u8arr(buf, len);
      for (let i = 0; i < len; i += 65536) {
        crypto.getRandomValues(view.subarray(i, Math.min(i + 65536, len)));
      }
      return E.SUCCESS;
    },

    sched_yield() { return E.SUCCESS; },
    fd_fdstat_set_flags() { return E.SUCCESS; },
    fd_fdstat_set_rights() { return E.SUCCESS; },
    fd_sync() { return E.SUCCESS; },
    fd_datasync() { return E.SUCCESS; },
    fd_advise() { return E.SUCCESS; },

    proc_exit(code) {
      throw new Error(`proc_exit(${code}) from within the alc_ng module`);
    },
  };

  const AlcWasi_ENV = ["TMPDIR=/tmp", "PWD=/", "PATH=/usr/local/bin:/bin"];

  // --- module lifecycle ------------------------------------------------------
  async function load(bytes, opts = {}) {
    const trace = !!opts.trace;
    const mod = await WebAssembly.compile(bytes);
    const importObject = {};
    for (const imp of WebAssembly.Module.imports(mod)) {
      if (imp.kind !== "function") {
        throw new Error(`unexpected wasm import: ${imp.module}.${imp.name} (${imp.kind})`);
      }
      const name = imp.name;
      const fn = wasiFns[name];
      let impl = fn;
      if (!impl) {
        impl = function () {
          console.warn(`[alc-wasi] unimplemented import: ${imp.module}.${name}`);
          return 51;  // ENOSYS
        };
      } else if (trace) {
        const label = name;
        // (fn-name → index of path ptr/len in the arg list) per the p1 ABI.
        const PATHY = {
          path_open: [2, 3], path_filestat_get: [2, 3],
          path_create_directory: [1, 2], path_remove_directory: [1, 2],
          path_unlink_file: [1, 2], path_rename: [1, 2],
        };
        impl = function (...args) {
          const pl = PATHY[label];
          const shown = pl
            ? `fd${args[0]} "${readStr(args[pl[0]], args[pl[1]])}"`
            : args.slice(0, 2).map((a) => (typeof a === "bigint" ? String(a) + "n" : String(a))).join(", ");
          console.log(`[wasi] ${label}(${shown})`);
          const r = fn.apply(this, args);
          if (r !== 0 && r !== undefined) console.log(`[wasi]   -> errno ${r}`);
          return r;
        };
      }
      (importObject[imp.module] ||= {})[name] = impl;
    }
    inst = await WebAssembly.instantiate(mod, importObject).then((r) => r.instance ?? r);
    // Emscripten's MEMFS does not necessarily carry a /tmp; the pipeline's
    // temp-dir handling expects one (TMPDIR=/tmp).
    try { FS.mkdir("/tmp"); } catch (e) {}
    // The preopens must exist in the fd table so that fd_fdstat_get (and
    // friends) treat fd 3/4 as the directories wasi-libc believes them to be.
    fds.set(PREOPEN_ROOT, { stream: null, path: "/", dir: "/", append: false });
    fds.set(PREOPEN_CWD, { stream: null, path: fsCwd(), dir: fsCwd(), append: false });
    // Constructors (wasi-libc preopens/environ) never run on their own in a
    // cdylib; alc_ng_init runs __wasm_call_ctors exactly once.
    if (!ctorsRan && inst.exports.alc_ng_init) {
      try {
        inst.exports.alc_ng_init();
      } catch (e) {
        throw new Error(`alc_ng_init failed: ${String((e && e.stack) || e)}`);
      }
      ctorsRan = true;
    }
    return Object.keys(inst.exports);
  }

  function run(json) {
    if (!inst) throw new Error("alc_ng module not loaded — send alcng_load first");
    const bytes = new TextEncoder().encode(json);
    const ptr = inst.exports.alc_ng_alloc(bytes.length);
    u8arr(ptr, bytes.length).set(bytes);
    try {
      const ret = inst.exports.alc_ng_run(ptr, bytes.length);
      const out = readCStr(ret);
      inst.exports.alc_ng_free_string(ret);
      return out;  // JSON produced by the Rust side
    } catch (e) {
      // A trap (panic under panic=abort, proc_exit) leaves the instance's
      // allocator in an undefined state — force a reload for the next run.
      inst = null;
      ctorsRan = false;
      throw e;
    } finally {
      if (inst) inst.exports.alc_ng_free_buf(ptr, bytes.length);
      flushStdio();  // e.g. a panic message cut short by the trap
    }
  }

  function flushStdio() {
    for (const fd of [1, 2]) {
      const rest = stdioPending[fd];
      stdioPending[fd] = "";
      if (rest) (fd === 2 ? console.error : console.log).call(console, rest);
    }
  }

  function list(path) {
    const root = normalizePath(path);
    const info = analyze(root);
    if (!info.exists) throw new Error(`no such path: ${root}`);
    if (!info.isDirectory) {
      return [{ path: root, size: nodeSize(info.object) }];  // single file
    }
    const out = [];
    (function walk(dir) {
      for (const name of FS.readdir(dir)) {
        if (name === "." || name === "..") continue;
        const full = dir === "/" ? "/" + name : dir + "/" + name;
        const entry = analyze(full);
        if (!entry.exists) continue;
        if (entry.isDirectory) walk(full);
        else out.push({ path: full, size: nodeSize(entry.object) });
      }
    })(root);
    out.sort((a, b) => (a.path < b.path ? -1 : 1));
    return out;
  }

  function removeTree(path) {
    const full = normalizePath(path);
    const info = analyze(full);
    if (!info.exists) return false;
    if (!info.isDirectory) {
      FS.unlink(full);
      return true;
    }
    for (const name of FS.readdir(full)) {
      if (name === "." || name === "..") continue;
      removeTree((full === "/" ? "" : full) + "/" + name);
    }
    FS.rmdir(full);
    return true;
  }

  function readFile(path) {
    const full = normalizePath(path);
    if (!analyze(full).exists) throw new Error(`no such file: ${full}`);
    return FS.readFile(full);  // Uint8Array copy — safe to postMessage
  }

  function reply(result, cmd, extra) {
    self.postMessage(Object.assign({ result, cmd }, extra));
  }

  return {
    // Returns true when msg was one of the alcng_* commands (the patched
    // worker passes its Emscripten FS along).
    dispatch(msg, fs) {
      if (!msg || typeof msg.cmd !== "string" || !msg.cmd.startsWith("alcng")) return false;
      FS = fs;
      switch (msg.cmd) {
        case "alcng_load":
          load(msg.wasm, { trace: msg.trace }).then(
            (exports) => reply("ok", msg.cmd, { exports }),
            (err) => { inst = null; ctorsRan = false; reply("failed", msg.cmd, { error: String((err && err.stack) || err) }); }
          );
          return true;
        case "alcng_run":
          try { reply("ok", msg.cmd, { json: run(msg.json) }); }
          catch (e) { reply("failed", msg.cmd, { error: String((e && e.stack) || e) }); }
          return true;
        case "alcng_list":
          try { reply("ok", msg.cmd, { entries: list(msg.path) }); }
          catch (e) { reply("failed", msg.cmd, { error: String((e && e.stack) || e) }); }
          return true;
        case "alcng_rm":
          try { reply("ok", msg.cmd, { removed: removeTree(msg.path) }); }
          catch (e) { reply("failed", msg.cmd, { error: String((e && e.stack) || e) }); }
          return true;
        case "alcng_read":
          try { reply("ok", msg.cmd, { data: readFile(msg.path) }); }
          catch (e) { reply("failed", msg.cmd, { error: String((e && e.stack) || e) }); }
          return true;
        default:
          return false;
      }
    },
  };
})();
