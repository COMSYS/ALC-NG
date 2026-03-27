# ALC-NG

The project contains the sanitization tool **ALC-NG** to clean latex projects. It strips all unused files, comments, if statements and more from the project that is not required to generate the final pdf. The project is inspired by the  Google-maintained [`arxiv-latex-cleaner`](https://github.com/google-research/arxiv-latex-cleaner).

# Main Features

*Sorted by technology and endorsement by arXiv. None of these tools is able to reliably sanitize all test cases. Regardless, a subset of authors seems to apply them prior to submission.*

| Name | Technology | Claimed Features | Last Update | T1 | T2 | T3 | T4 | T5 | T6 | T7 | T8 | T9 | Beneficial | Breaks |
| :--- | :--- | :--- | :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| **perl one-liner** | regex-based | comments | 11/2005 | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | 95% | 4.5% |
| **arxiv_latex_cleaner (ALC)** | regex-based | dangling files | 06/2025 | ✅ | ❌ | ✅ | ❌ | ✅ | ✅ | ❌ | 🛠️ | ✅ | 91% | 19% |
| | | comments | | | | | | | | | | | *with manually-enabled command cleaning:* | 98% | 29% |
| **latexindent.pl** | regex-based | comments | 02/2026 | ✅ | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | 17% | 11% |
| **arXiv Cleaner** | pdflatex recorder | dangling files | 09/2019 | ✅ | ✅ | 💥 | ❌ | ✅ | ❌ | ❌ | ❌ | ✅ | 80% | 80% |
| | latexpand | comments | | *only latexpand:* | | | | | | | | | 69% | 85% |
| **Sub. Sanitizer & Flattener** | snapshot | dangling files | 06/2022 | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | *not reasonably applicable* | |
| **pandoc** | tokenization | n/a | 03/2026 | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ | ✅ | ✅ | ❌ | *not reasonably applicable* | |
| **ALC-NG** *(this project)* | pdflatex recorder | dangling files | 04/2026 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | 97% | 15% |
| | exiftool | metadata | | | | | | | | | | | | |
| | tree-sitter-based | comments | | | | | | | | | | | | |

### Legend & Notes

**Comment Cleanup Tests (T1–T9):**
1.  **Inline comment removal**
2.  **`\\%` detection**
3.  **Comment environment**
4.  **Retain comments within special environments**
5.  **Out of document removal**
6.  **`\iffalse`/`\if0` handling**
7.  **Custom `\if` removal**
8.  **No arg. command cleaning**
9.  **Bbl file support**

**Symbol Key:**
*   ✅ **Cleans** \LaTeX{} source files successfully
*   ❌ **Unsuccessful** (attempt)
*   🛠️ **Manual** action required for sanitization
*   💥 **Crash** (tool crashes, ie failed test)

**Footnotes:**
*   `^§` Breaks defined as: (i) tool crashes, (ii) paper cannot compile, or (iii) mismatch in pixel-perfect comparison.
*   Mapped to concepts: A (T1–T4); B (T5); C (T6–T7); D (T8).

## Reliable Analysis of Unused Content

- The tool requires a working latex installation to compile the document and determine unused files.
- We use [Tree-Sitter](https://tree-sitter.github.io/tree-sitter/) to parse each latex file and strip unused parts
- The implementation has been tested on over XX arxiv papers with a success rate of XX

## Remove Sensitive Information

- We use exiftool to strip metadata from images and pdfs.

## Optimize Size

- The tool allows to resize images to a predefined size and format
- Unused content is removed
- Per default we only keep the generated .bbl file for bibliographies

## Validation

- The tool can make a pixel-perfect comparison between the original and cleaned version to ensure that it did not break the project.

## arXiv ready

- The cleaned project can directly be packaged into a `.tar.gz` archive, ready to upload to arXiv.

# Installation

## Cargo

Assuming a working Rust and Cargo installation, you can install the cleaner using `cargo install` with the following command:

```bash
cargo install --git https://github.com/COMSYS/ALC-NG.git
```

## APT

## Homebrew

## Winget

## Docker

# Usage

## Basic Usage

```bash
alc-ng <INPUT_PATH> [OPTIONS]
```

The tool takes an input path to your LaTeX project and outputs cleaned files to a target directory (default: `./cleaned`).

## Command-Line Options

| Option | Shorthand | Default | Description |
|--------|-----------|---------|-------------|
| `[first arg]` | - | (required) | Path to the LaTeX project directory to clean |
| `[second arg]` | - | `./cleaned` | Directory where cleaned files will be written |
| `--latex-cmd` | - | `latexmk` | LaTeX compiler command to use (e.g., `latexmk`, `pdflatex`) |
| `--latex-args` | - | - | Additional command-line parameters passed to the LaTeX compiler |
| `--verbose` | `-v` | `false` | Enable debug-level logging |
| `--compare` | `-c` | `false` | Compile the cleaned folder and compare PDF output with originals |
| `--force` | `-f` | `false` | Continue cleaning despite hitting errors |
| `--exiftool-cmd` | - | `exiftool` | Command for exiftool (used for metadata stripping) |
| `--exiftool-args` | - | - | Additional parameters to pass to exiftool |
| `--strip-exif` | - | `false` | Strip EXIF metadata from images and PDFs using exiftool |
| `--keep-bib` | - | `false` | Preserve `.bib` bibliography files (default: only keeps generated `.bbl`) |
| `--resize-images` | - | `false` | Resize images to reduce file size |
| `--im-size` | - | `512` | Target image size in pixels (longest side) when resizing |
| `--clean-classes` | - | `false` | Also clean `.sty` and `.cls` LaTeX class/style files |
| `--no-zzrm` | - | `false` | Ignore `00readme` file even if defined in the project |
| `--tar` | - | - | Create a `.tar.gz` archive ready for arXiv upload (use `-` to pipe to stdout) |
| `--skip-watermark` | - | `false` | Skip adding watermark to cleaned files |

## Examples

### Basic cleaning

Clean a LaTeX project in the current directory, outputting to `./cleaned`:

```bash
alc-ng ./my-latex-project
```

### Clean with verbose output

Enable debug logging to see detailed processing information:

```bash
alc-ng ./my-latex-project -v
```

### Clean and validate

Clean the project and verify the output PDF matches the original:

```bash
alc-ng ./my-latex-project -c
```

### Clean with image optimization

Strip metadata and resize images to reduce file size:

```bash
alc-ng ./my-latex-project --strip-exif --resize-images --im-size 1024
```

### Preserve bibliography files

Keep the original `.bib` files instead of only the generated `.bbl`:

```bash
alc-ng ./my-latex-project --keep-bib
```

### Clean with custom LaTeX compiler

Use `pdflatex` instead of the default `latexmk`:

```bash
alc-ng ./my-latex-project --latex-cmd pdflatex
```

### Create arXiv submission archive

Clean the project and package it as a `.tar.gz` archive:

```bash
alc-ng ./my-latex-project --tar submission.tar.gz
```

Pipe the archive to stdout (useful for CI/CD pipelines):

```bash
alc-ng ./my-latex-project --tar - > submission.tar.gz
```

### Clean style and class files

Also process `.sty` and `.cls` files in the project:

```bash
alc-ng ./my-latex-project --clean-classes
```

### Force mode

Continue processing even if errors are encountered:

```bash
alc-ng ./my-latex-project -f
```

### Custom output directory

Specify a custom target directory for cleaned files:

```bash
alc-ng ./my-latex-project --target-path ./cleaned-output
```

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 72 | Grammar/parsing warnings detected (check output) |
| >0 | Error occurred during processing |
