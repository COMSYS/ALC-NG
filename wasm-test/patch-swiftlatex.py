"""Applies the alc-ng test-page patches to the stock SwiftLaTeX XeTeX and
dvipdfmx worker glue (see wasm-test/swiftlatex/README.md).

usage: python3 -I wasm-test/patch-swiftlatex.py <extracted release dir> wasm-test/swiftlatex
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

DISPATCH_OLD = 'cmd==="flushcache"){cleanDir(WORKROOT)}else{'
DISPATCH_XETEX = (
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

SEED_OLD = "if(cacheKey in texlive404_cache){return 0}if(cacheKey in texlive200_cache)"
SEED_NEW = (
    "if(self.texFiles&&reqname in self.texFiles){if(!(cacheKey in texlive200_cache)){"
    'const seedpath=TEXCACHEROOT+"/seed_"+reqname.replace(/\\//g,"_");'
    "FS.writeFile(seedpath,self.texFiles[reqname]);texlive200_cache[cacheKey]=seedpath}"
    'return allocate(intArrayFromString(texlive200_cache[cacheKey]),"i8",ALLOC_NORMAL)}'
    + SEED_OLD
)

OPEN_OLD = "try{var pathname=SYSCALLS.getStr(path);"
OPEN_NEW = OPEN_OLD + "(self.openLog||(self.openLog=[])).push({p:pathname,m:flags&3});"


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


def patch_cd(text, routine):
    for old, new in [
        (SETMAIN_OLD, SETMAIN_NEW),
        (OUTURL_OLD, OUTURL_NEW),
        chdir_patch(routine),
    ]:
        text = patch(text, old, new)
    return text


def patch(text, old, new):
    assert text.count(old) == 1, f"patch anchor not unique: {old[:60]}"
    return text.replace(old, new)


xetex = (src / "swiftlatexxetex.js").read_text()
for old, new in [
    (MKDIR_OLD, MKDIR_NEW),
    (DISPATCH_OLD, DISPATCH_XETEX),
    (SEED_OLD, SEED_NEW),
    (OPEN_OLD, OPEN_NEW),
]:
    xetex = patch(xetex, old, new)
xetex = patch_cd(xetex, "compileLaTeXRoutine")
(out / "swiftlatexxetex.js").write_text(
    'importScripts("alc-wasi.js");\n' + CONSOLE + xetex
)

dvi = (src / "swiftlatexdvipdfm.js").read_text()
dvi = patch(dvi, MKDIR_OLD, MKDIR_NEW)
dvi = patch_cd(dvi, "compilePDFRoutine")
(out / "swiftlatexdvipdfm.js").write_text(CONSOLE + dvi)
print("patched swiftlatexxetex.js and swiftlatexdvipdfm.js")

# The pdfTeX glue carries older, hand-applied patches (README); only the -cd
# patch is scripted. Applied in place, once.
pdftex_path = out / "swiftlatexpdftex.js"
pdftex = pdftex_path.read_text()
if "self.texdir" not in pdftex:
    pdftex_path.write_text(patch_cd(pdftex, "compileLaTeXRoutine"))
    print("patched swiftlatexpdftex.js (-cd)")
