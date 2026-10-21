"""Applies the alc-ng test-page patches to the stock SwiftLaTeX worker glue
(see wasm-test/swiftlatex/README.md), writing all three patched workers.

usage: python3 -I wasm-test/patch-swiftlatex.py <extracted release dir> wasm-test/swiftlatex

Everything the test page needs on top of the release is here: no worker glue is
patched by hand anymore, so a fresh checkout is reproducible with the release
zip plus this script. `alc-wasi.js` (the WASI shim the alcng_* commands run in)
is tracked next to the README, and the pdfTeX and XeTeX glue `importScripts`
it.
"""

import sys
from pathlib import Path

src, out = Path(sys.argv[1]), Path(sys.argv[2])

CONSOLE = """/*alc-ng patch: forward worker console to the host page*/
(function(){
    const send=function(lvl,text){try{self.postMessage({cmd:"workerlog",level:lvl,text:text})}catch(e){}};
    ["log","info","warn","error"].forEach(function(lvl){
        const orig=console[lvl].bind(console);
        console[lvl]=function(){const a=Array.prototype.slice.call(arguments);send(lvl,a.map(function(x){return typeof x==="string"?x:JSON.stringify(x)}).join(" "));orig.apply(null,a)};
    });
})();
"""

MKDIR_OLD = 'function mkdirRoutine(dirname){try{FS.mkdir(WORKROOT+"/"+dirname);'
MKDIR_NEW = (
    'function mkdirRoutine(dirname){const path=WORKROOT+"/"+dirname;'
    "try{try{FS.mkdir(path)}catch(e){if(!FS.analyzePath(path).exists){throw e}}"
)

# Host-side commands the stock worker does not have: drain the file access log
# (openlog, README patch 4), route the alc-ng sidecar (AlcWasi, patch 5), and
# inject/inspect files (seedfiles/listdir/readfile, patches 1-2).
DISPATCH_OLD = 'cmd==="flushcache"){cleanDir(WORKROOT)}else{'
DISPATCH_NEW = (
    'cmd==="flushcache"){cleanDir(WORKROOT)}else if(cmd==="openlog"){const e=self.openLog||[];'
    'self.openLog=[];self.postMessage({result:"ok",cmd:"openlog",entries:e})}'
    "else if(self.AlcWasi&&AlcWasi.dispatch(data,FS)){}"
    'else if(cmd==="seedfiles"){self.texFiles=data["files"];self.postMessage({result:"ok",cmd:"seedfiles"})}'
    'else if(cmd==="listdir"){try{let names=FS.readdir(WORKROOT);self.postMessage({result:"ok",cmd:"listdir",names:names})}'
    'catch(e){self.postMessage({result:"failed",cmd:"listdir"})}}'
    'else if(cmd==="readfile"){try{let c=FS.readFile(WORKROOT+"/"+data["url"],{encoding:"binary"});'
    'self.postMessage({result:"ok",cmd:"readfile",url:data["url"],content:c.buffer},[c.buffer])}'
    'catch(e){self.postMessage({result:"failed",cmd:"readfile",url:data["url"],error:String(e?.message||e)})}}else{'
)

# patch 3: the kpathsea hook serves seeded files before anything is fetched.
SEED_OLD = "if(cacheKey in texlive404_cache){return 0}if(cacheKey in texlive200_cache)"
SEED_NEW = (
    "if(self.texFiles&&reqname in self.texFiles){if(!(cacheKey in texlive200_cache)){"
    'const seedpath=TEXCACHEROOT+"/seed_"+reqname.replace(/\\//g,"_");'
    "FS.writeFile(seedpath,self.texFiles[reqname]);texlive200_cache[cacheKey]=seedpath}"
    'return allocate(intArrayFromString(texlive200_cache[cacheKey]),"i8",ALLOC_NORMAL)}'
    + SEED_OLD
)

# patch 4: record every open, so the page can build the used-file list from it.
OPEN_OLD = "try{var pathname=SYSCALLS.getStr(path);"
OPEN_NEW = OPEN_OLD + "(self.openLog||(self.openLog=[])).push({p:pathname,m:flags&3});"

# compileformat reads back `pdflatex.fmt`, but the dump it just wrote is named
# after the kpathsea cache id of pdflatex.ini; the page seeds it back under the
# name the engine expects, so it has to ask for that one.
FMT_OLD = 'WORKROOT+"/pdflatex.fmt"'
FMT_NEW = 'WORKROOT+"/swiftlatexpdftex.fmt"'


# latexmk -cd: `setmainfile` may name the main file's directory (`dir`,
# relative to /work); the compile then runs there and reads its output from
# there, so `../figs/...` resolves like in a native compile.
SETMAIN_OLD = 'cmd==="setmainfile"){self.mainfile=data["url"]}'
SETMAIN_NEW = (
    'cmd==="setmainfile"){self.mainfile=data["url"];'
    'self.texdir=data["dir"]?WORKROOT+"/"+data["dir"]:WORKROOT}'
)
OUTURL_OLD = 'WORKROOT+"/"+self.mainfile'
OUTURL_NEW = '(self.texdir||WORKROOT)+"/"+self.mainfile'


def chdir_patch(routine):
    old = f"function {routine}(){{prepareExecutionContext();"
    return old, old + "FS.chdir(self.texdir||WORKROOT);"


def patch(text, old, new):
    assert text.count(old) == 1, f"patch anchor not unique: {old[:60]}"
    return text.replace(old, new)


def patch_cd(text, routine):
    for old, new in [
        (SETMAIN_OLD, SETMAIN_NEW),
        (OUTURL_OLD, OUTURL_NEW),
        chdir_patch(routine),
    ]:
        text = patch(text, old, new)
    return text


def worker(name, patches, cd_routine, sidecar):
    """Patches one worker glue and writes it; `sidecar` loads alc-wasi.js."""
    text = (src / f"{name}.js").read_text()
    for old, new in patches:
        text = patch(text, old, new)
    text = patch_cd(text, cd_routine)
    prefix = ('importScripts("alc-wasi.js");\n' if sidecar else "") + CONSOLE
    (out / f"{name}.js").write_text(prefix + text)
    print(f"patched {name}.js")


HOST_COMMANDS = [
    (MKDIR_OLD, MKDIR_NEW),
    (DISPATCH_OLD, DISPATCH_NEW),
    (SEED_OLD, SEED_NEW),
    (OPEN_OLD, OPEN_NEW),
]

worker(
    "swiftlatexpdftex",
    HOST_COMMANDS + [(FMT_OLD, FMT_NEW)],
    "compileLaTeXRoutine",
    sidecar=True,
)
worker("swiftlatexxetex", HOST_COMMANDS, "compileLaTeXRoutine", sidecar=True)
worker(
    "swiftlatexdvipdfm", [(MKDIR_OLD, MKDIR_NEW)], "compilePDFRoutine", sidecar=False
)
