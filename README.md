# ALC-NG

A modern LaTeX sanitization tool for arXiv submissions. Strips unused files, comments, conditionals, and metadata while preserving the same output PDF.
Inspired by [`arxiv_latex_cleaner`](https://github.com/google-research/arxiv-latex-cleaner) from Google.

[![Paper](https://img.shields.io/badge/arXiv-2604.XXX-b31b1b.svg)](https://arxiv.org/2604.XXX)
[![Zenodo](https://img.shields.io/badge/Zenodo-Artifact-1682d4.svg)](https://zenodo.org/record/19366799)
[![Website](https://img.shields.io/badge/Project-Website-0a7d00.svg)](https://arxiv.comsys.rwth-aachen.de)

> [!CAUTION]
> Use at your own risk. No warranty for functionality, stability, or data safety.

## Table of Contents

- [Quick Start](#quick-start)
- [Installation](#installation)
- [Usage](#usage)
- [How It Works](#how-it-works)
- [Comparison with Other Tools](#comparison-with-other-tools)
- [Known Constraints & Future Work](#known-constraints--future-work)
- [Publication](#publication)
- [License](#license)

## Quick Start

```bash
# Install (requires Rust, clang, and Node.js; see Installation)
cargo install --git https://github.com/COMSYS/ALC-NG.git

# Basic cleaning (output is written to ./cleaned by default)
alc-ng ./my-latex-project

# Clean and validate that the output PDF matches the original
alc-ng ./my-latex-project -c

# Clean and create an arXiv-ready archive
alc-ng ./my-latex-project --tar submission.tar.gz
```

## Installation

### Requirements

**Build-time** (to compile `alc-ng`):
- [Rust](https://www.rust-lang.org/tools/install) (stable toolchain, incl. Cargo)
- `clang` (required by the tree-sitter build)
- [Node.js](https://nodejs.org/) (required to generate the tree-sitter grammar)

**Runtime** (to run `alc-ng`):
- A working **local TeX distribution** (e.g. TeX Live, MacTeX). ALC-NG invokes a LaTeX compiler (`latexmk` by default) to determine which files are actually used, and, when `--compare` is enabled, to recompile the cleaned output for validation.
- Optional: [`exiftool`](https://exiftool.org/) if you want to strip image and PDF metadata via `--strip-exif`.

### Prebuild binaries

Check the [GitHub releases page](https://github.com/COMSYS/ALC-NG/releases).

> [!NOTE]
> We provide signed binaries for macOS. As they are signed manually, you might need to wait a while to see in them for a new release.
> All other published binaries are currently **unsigned**. Code signing for Windows is in progress.
> Cautious users should [build from source](#from-source-cargo) instead.

### From Source (Cargo)

```bash
cargo install --git https://github.com/COMSYS/ALC-NG.git
```

> [!WARNING]
> The build also compiles the tree-sitter grammar and can therefore take a few minutes. This is expected.

## Usage

```bash
alc-ng [OPTIONS] <INPUT_PATH> [<OUTPUT_PATH>]
```

`<INPUT_PATH>` points to your LaTeX project directory. Cleaned files are written to `<OUTPUT_PATH>` (default `./cleaned`).

### Common Examples

```bash
# Clean and validate the output PDF against the original
alc-ng ./my-latex-project -c

# Clean with verbose debug logging
alc-ng ./my-latex-project -v

# Strip image metadata and downscale images
alc-ng ./my-latex-project --strip-exif --resize-images --im-size 1024

# Produce a ready-to-upload arXiv archive
alc-ng ./my-latex-project --tar submission.tar.gz

# Force continuation on errors, keep .bib files, also clean .sty/.cls
alc-ng ./my-latex-project --keep-bib --clean-classes --latex-cmd pdflatex -f
```

### Command-Line Options

| Option | Short | Default | Description |
|---|---|---|---|
| `<INPUT_PATH>` | | *(required)* | LaTeX project directory to clean |
| `<OUTPUT_PATH>` | | `./cleaned` | Destination directory for cleaned files |
| `--compare` | `-c` | `false` | Recompile the cleaned project and pixel-compare PDFs against the original. This will produce image diffs of pages that are not identical. |
| `--verbose` | `-v` | `false` | Enable debug logging |
| `--force` | `-f` | `false` | Continue on recoverable errors |
| `--main-files` | `-m` | | Manually provide a list of main tex files as compile entrypoints.<br>The cleaner can usually infer main tex files automatically. |
| `--keep-bib` | | `false` | Keep `.bib` files (by default only the generated `.bbl` is kept) |
| `--clean-classes` | | `false` | Also clean `.sty` and `.cls` files |
| `--latex-cmd` | | `latexmk` | LaTeX compiler to invoke (e.g. `latexmk`, `pdflatex`) |
| `--latex-args` | | | Extra arguments passed to the LaTeX compiler |
| `--exiftool-cmd` | | `exiftool` | exiftool binary to use |
| `--exiftool-args` | | | Extra arguments passed to exiftool |
| `--strip-exif` | | `false` | Strip EXIF metadata from images and PDFs |
| `--resize-images` | | `false` | Downscale images to reduce file size |
| `--im-size` | | `512` | Target image size in pixels (longest side) when resizing |
| `--no-zzrm` | | `false` | Ignore an existing `00readme` file |
| `--tar` | | | Write a `.tar.gz` archive (use `-` to pipe to stdout) |
| `--skip-watermark` | | `false` | Do not add a watermark to cleaned files |
| `--diff-color` | | `ff0000` | Provide a color for pixel that have changed during the pixel-perfect compare. |

## How It Works

ALC-NG combines several techniques to sanitize LaTeX projects while preserving the compiled output:

- **Reliable unused-content analysis.** Compiles the project with a local LaTeX installation to determine which files are actually used, then parses each LaTeX file with [Tree-Sitter](https://tree-sitter.github.io/tree-sitter/) to strip unused parts. Tested on all 2.8M arXiv papers with an 85% success rate (as of December 2025, `alc-ng` 0.1.0).
- **Sensitive metadata removal.** Optionally uses exiftool to strip metadata from images and PDFs.
- **Size optimization.** Resizes images to a configurable size and format; by default keeps only the generated `.bbl` instead of all `.bib` files.
- **Validation.** Performs a pixel-perfect comparison between the original and cleaned PDFs to confirm correctness.
- **arXiv-ready packaging.** Cleaned projects can be emitted directly as `.tar.gz` archives.

## Comparison with Other Tools

*Sorted by technology and endorsement by arXiv. None of these tools reliably sanitizes all test cases, but a subset of authors nonetheless apply them prior to submission.*

| Name | Claimed Features | T1 | T2 | T3 | T4 | T5 | T6 | T7 | T8 | T9 |
| :--- | :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| **[perl one-liner](https://info.arxiv.org/help/faq/whytex.html)** | comments | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| **[arxiv_latex_cleaner](https://github.com/google-research/arxiv-latex-cleaner)** | dangling files<br>comments | ✅ | ❌ | ✅ | ❌ | ✅ | ✅ | ❌ | 🛠️ | ✅ |
| **[latexindent.pl](https://github.com/cmhughes/latexindent.pl)** | comments | ✅ | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| **[arXiv Cleaner](https://github.com/elsa-lab/arxiv-cleaner)** | dangling files<br>comments | ✅ | ✅ | 💥 | ❌ | ✅ | ❌ | ❌ | ❌ | ✅ |
| **[Sub. Sanitizer & Flattener](https://github.com/davidstutz/arxiv-submission-sanitizer-flattener)** | dangling files | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| **[pandoc](https://pandoc.org/)** | n/a | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ | ✅ | ✅ | ❌ |
| **ALC-NG** *(this project)* | dangling files<br>metadata<br>comments | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |

**Comment cleanup tests (T1 to T9):**
1. Inline comment removal
2. `\%` detection
3. `comment` environment
4. Retain comments within special environments
5. Out-of-document removal
6. `\iffalse` / `\if0` handling
7. Custom `\if` removal
8. No-argument command cleaning
9. `.bbl` file support

**Symbol key:** ✅ cleans successfully. ❌ unsuccessful. 🛠️ manual action required. 💥 tool crashes.

## Known Constraints & Future Work

### Bib file cleaning

We currently do not support stripping unused entries from bibliographies. The default action is to only preserve the `.bbl` file that suffices to compile the document. You can use the `--keep-bib` flag to also preserve the original `.bib` file when cleaning.

We are planning to add support for this [later](#possible-enhancements).

### Supported comment types

We try to remove as much (unneeded) information as possible. We also evaluate ifs and consider custom command comments (command definitions with empty body). Below you can see examples of how the cleaner handles different cases.

> [!NOTE]
> We cannot replicate every custom control flow logic. If you use custom control flows, make sure that the cleaner has picked it up or that you evaluate it before passing the source code to the cleaner.

```latex
\documentclass[12pt]{article}
\usepackage{comment}
\usepackage{listings}

\newcommand{\customcmd}[1]{custom:~#1}
\renewcommand{\customcmd}[1]{} % ignore the argument (8)

\newif\iffoo
\foofalse % set iffoo to false

% (5) out of document removal: Should be gone

\begin{document}
% (1) inline comment removal (1/2): Should be gone
Hello World! \cite{latex2e}% (1) inline comment removal (2/2): Part after percent should be gone

% comment after % Should be gone
20\textbackslash\% just a percent: Should remain
new line \\%still a comment (2) doublebackslash percent detection: Should be gone

\begin{comment}
comment without percent (3) comment env.: Should be gone
\end{comment}

\begin{verbatim}
\% not a comment (4) special env.: Should remain
\end{verbatim}

\begin{lstlisting}
int a = 2 \% 1 \# ---"--- (4) special env.: Should remain
\end{lstlisting}

% (6) iffalse removal test cases
\iffalse a. should be gone \fi
\if0 -. should be gone \fi
\iftrue 1. should remain \fi
\iffalse b. should be gone
\else 2. should remain \fi
\if0 -. should be gone
\else +. should remain \fi
\iftrue 3. should remain
\else c. should be gone \fi

% (7) custom if clause removal test cases
\iffoo should be gone too \fi
\iffoo should be gone too
\else should remain too \fi

% the next line requires (8) no argument command cleaning
\customcmd{just a comment: Should be gone}

\bibliographystyle{plain} % Relevant for (9)
\bibliography{example} % Relevant for (9)

\end{document}
(5) out of document removal: Should be gone
```

### No metadata cleaning with default settings

Due to the way that removing metadata can affect the rendering of images (e.g. rotation and color), it is disabled by default. The comapre feature will almost always report differences if the `--strip-exif` flag is used. Please verify manually that the resulting pdf still looks acceptable to you. Removing metadata is an important part to remove privacy and security sensitve information (see our paper for more information).

### No cleaning of class and style files

We currently only clean `.tex` files by default. You can turn on cleaning of other latex-like files (like `.sty` and `.cls`) with the `--clean-classes` flag. Class files usually do not contain any sensitve information and are made up of complex latex code which often result in grammar parsing errors. Depending on the result error, the cleaned file can be broken. This is for example the case for the IEEE class file.

### Supported new-command definitions

<details>
<summary>Click to expand the full list</summary>

```latex
\newcommand                         \DeclareRobustCommand
\newcommand*                        \DeclareRobustCommand*
\renewcommand                       \DeclareMathOperator
\renewcommand*                      \DeclareMathOperator*
\providecommand                     \NewDocumentCommand
\providecommand*                    \RenewDocumentCommand
\ProvideDocumentCommand             \DeclareDocumentCommand
\NewExpandableDocumentCommand       \RenewExpandableDocumentCommand
\ProvideExpandableDocumentCommand   \DeclareExpandableDocumentCommand
\NewCommandCopy                     \RenewCommandCopy
\DeclareCommandCopy                 \def
\gdef                               \edef
\xdef
```

See the [tree-sitter grammar file](https://github.com/COMSYS/ALC-NG/blob/094e8ff48b58ee156022e2cb7fde0314761d846f/external/ts-latex/grammar.js#L1110) for the full list.

</details>

### Possible enhancements

The following items are known limitations we would like to address in future versions:

- **Citation-key and label anonymization.** Replace user-chosen `\cite{...}` keys and `\label{...}` identifiers with neutral placeholders.
- **Deeper `.bib` cleanup.** Keep only entries actually referenced by the document, and optionally strip unused custom fields.
- **Directory flattening.** Collapse the project's directory structure and reduce the number of TeX files to the necessary minimum.
- **Unused packages.** Detect and remove `\usepackage{...}` lines whose macros are never used.
- **Unused imports.** Drop `\input{}` / `\include{}` references to files that do not contribute to the final document.
- **Default `00README` generation.** Auto-generate a sensible default `00README` when none is present.

## Publication

If you use any portion of this work, please cite our paper:

> Jan Pennekamp, Johannes Lohmöller, David Schütte, Joscha Loos, Martin Henze.
> **Hidden Secrets in the arXiv. Discovering, Analyzing, and Preventing Unintentional Information Disclosure in Source Files of Scientific Preprints.**
> In *Proceedings of IEEE S&P 2026*, San Francisco, CA, May 18-21, 2026.

```bibtex
@inproceedings{pennekamp2026arxiv,
  author    = {Pennekamp, Jan and Lohm{\"o}ller, Johannes and Sch{\"u}tte, David and Loos, Joscha and Henze, Martin},
  title     = {Hidden Secrets in the arXiv. Discovering, Analyzing, and Preventing Unintentional Information Disclosure in Source Files of Scientific Preprints},
  booktitle = {Proceedings of the 47th IEEE Symposium on Security and Privacy (S\&P '26)},
  year      = {2026},
  address   = {San Francisco, CA, USA},
  month     = may,
}
```

Related resources:

- [![arXiv](https://img.shields.io/badge/arXiv-2604.XXX-b31b1b.svg)](https://arxiv.org/2604.XXX) Paper preprint
- [![Zenodo](https://img.shields.io/badge/Zenodo-Artifact-1682d4.svg)](https://zenodo.org/record/19366799) Reproducibility artifact
- [![Website](https://img.shields.io/badge/Project-Website-0a7d00.svg)](https://arxiv.comsys.rwth-aachen.de) Project website

## License

ALC-NG is released under the MIT License. See [LICENSE](LICENSE) for the full text.

Copyright © 2026 COMSYS, RWTH Aachen University, and the authors.
