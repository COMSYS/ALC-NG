mod common;

#[cfg(test)]
mod tests {
    use crate::common::{is_pixel_identical, run_cleaner, test_output_dir};

    use alc_ng::cleaner::config::CleanerConfig;
    use alc_ng::cleaner::file_renames::FileRenames;
    use alc_ng::cleaner::submission::Submission;
    use alc_ng::cleaner::submission::parsed_file::ContentStripper;
    use alc_ng::helper::ContainsByteSlice;
    use alc_ng::parsing::parse;
    use flate2::read::GzDecoder;
    use std::fs::File;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    fn clean(name: &str, tar: Option<&Path>) -> (Submission, PathBuf) {
        let config = Arc::new(CleanerConfig {
            force: true,
            skip_watermark: true,
            tar: tar.map(|t| t.to_string_lossy().to_string()),
            rename_files: true,
            ..Default::default()
        });

        let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
        let test_files = root.join("tests/spaces");
        let target_path = test_output_dir().join(name);
        let mut submission =
            Submission::new(config, test_files, &target_path, "latexmk", &vec![]).unwrap();

        submission.run().unwrap();

        (submission, target_path)
    }

    fn read(path: impl AsRef<Path>) -> Vec<u8> {
        std::fs::read(path).unwrap()
    }

    #[test]
    fn replaces_spaces_in_file_names() {
        let tar = test_output_dir().join("alc-ng-spaces-test.tar.gz");
        let (submission, target) = clean("spaces_cleaned", Some(&tar));

        assert!(target.join("main_paper.tex").is_file());
        assert!(target.join("intro_section.tex").is_file());
        assert!(target.join("my_sections/part_a.tex").is_file());
        assert!(target.join("figs/red_square.png").is_file());
        assert!(target.join("figs/blue_square.png").is_file());
        assert!(target.join("my_figs/square.png").is_file());
        assert!(!target.join("main paper.tex").exists());
        assert!(!target.join("my sections").exists());
        assert!(!target.join("my figs").exists());

        let main = read(target.join("main_paper.tex"));
        assert!(main.contains_slice(br"\input{intro_section}"));
        assert!(main.contains_slice(br"\input{my_sections/part_a}"));
        assert!(main.contains_slice(br"\includegraphics[width=1cm]{figs/red_square.png}"));
        assert!(main.contains_slice(br"\includegraphics[width=1cm]{blue_square}"));
        assert!(main.contains_slice(br"\graphicspath{{figs/}{my_figs/}}"));
        assert!(main.contains_slice(br"\includegraphics[width=1cm]{square}"));
        assert!(!main.contains_slice(b"A comment"));

        // Files that are not cleaned only get their references rewritten
        let frozen = read(target.join("frozen.tex"));
        assert_eq!(
            frozen,
            b"% !ALC-NG keep-all\nFrozen: \\input{intro_section} % this comment is kept\n"
        );
        let logo = read(target.join("logo.sty"));
        assert!(logo.contains_slice(b"% A comment in an uncleaned package"));
        assert!(logo.contains_slice(br"\includegraphics[width=1cm]{figs/red_square}"));

        // The tar archive contains the renamed files
        let mut archive = tar::Archive::new(GzDecoder::new(File::open(&tar).unwrap()));
        let entries: Vec<String> = archive
            .entries()
            .unwrap()
            .map(|e| e.unwrap().path().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(entries.iter().any(|e| e == "main_paper.tex"));
        assert!(entries.iter().any(|e| e == "my_sections/part_a.tex"));
        assert!(entries.iter().all(|e| !e.contains(' ')));
        std::fs::remove_file(&tar).unwrap();

        // The renamed project compiles to the same PDF
        assert!(is_pixel_identical(&submission));
    }

    #[test]
    fn rewrites_references_of_search_paths_and_package_commands() {
        let renames = FileRenames::new(
            [
                "my figs/plot.png",
                "my refs.bib",
                "my doc.pdf",
                "code/my script.py",
            ]
            .iter()
            .map(Path::new),
        );
        let input = br"\graphicspath{{figs/}{my figs/}}
\addbibresource{my refs.bib}
\bibliography{my refs,other}
\includepdf[pages=-]{my doc}
\lstinputlisting[language=Python]{code/my script.py}
\includegraphics{../my figs/plot}
";
        let tree = parse(input).unwrap();
        let (stripped, stats) = ContentStripper::clean_with_renames(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
            Some(&renames),
        )
        .unwrap();

        assert_eq!(
            stripped.as_deref(),
            Some(
                &br"\graphicspath{{figs/}{my_figs/}}
\addbibresource{my_refs.bib}
\bibliography{my_refs,other}
\includepdf[pages=-]{my_doc}
\lstinputlisting[language=Python]{code/my_script.py}
\includegraphics{../my_figs/plot}
"[..]
            )
        );
        assert!(stats.grammar_errors.is_empty());
    }

    /// With `clean_classes`, package files are cleaned and their references rewritten.
    #[test]
    fn cleans_and_rewrites_class_files() {
        let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
        let (submission, target, result) = run_cleaner(
            "spaces_clean_classes_cleaned",
            &root.join("tests/spaces"),
            CleanerConfig {
                force: true,
                skip_watermark: true,
                rename_files: true,
                clean_classes: true,
                ..Default::default()
            },
        );
        result.unwrap();

        let logo = read(target.join("logo.sty"));
        assert!(!logo.contains_slice(b"% A comment in an uncleaned package"));
        assert!(logo.contains_slice(br"\includegraphics[width=1cm]{figs/red_square}"));

        assert!(is_pixel_identical(&submission));
    }

    /// Only file references are rewritten, not text that happens to match a renamed file.
    #[test]
    fn does_not_rewrite_text_that_is_not_a_reference() {
        let renames = FileRenames::new(
            ["figs/blue square.png", "intro section.tex"]
                .iter()
                .map(Path::new),
        );
        let input = br"\emph{blue square} \label{fig:blue square}
\begin{verbatim}
\input{intro section}
\end{verbatim}
";
        let tree = parse(input).unwrap();
        let (stripped, _) = ContentStripper::clean_with_renames(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
            Some(&renames),
        )
        .unwrap();

        assert_eq!(stripped.as_deref(), Some(&input[..]));

        // Comments are not references either, also in files that are not cleaned
        let uncleaned = b"% \\input{intro section}\n\\emph{blue square}\n";
        let tree = parse(uncleaned).unwrap();
        assert_eq!(
            renames.rewrite_references(uncleaned, tree.root_node(), "test.tex"),
            None
        );
    }

    /// References in the body of a command definition are rewritten in cleaned files,
    /// with and without an argument count.
    #[test]
    fn rewrites_references_inside_command_definitions() {
        let renames = FileRenames::new(["figs/red square.png"].iter().map(Path::new));
        let input = br"\newcommand{\logo}{\includegraphics{figs/red square}}
\renewcommand{\logo}{\includegraphics{figs/red square}}
\newcommand{\icon}[1]{\includegraphics[width=#1]{figs/red square}}
\def\badge{\includegraphics{figs/red square}}
";
        let tree = parse(input).unwrap();
        let (stripped, _) = ContentStripper::clean_with_renames(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
            Some(&renames),
        )
        .unwrap();

        assert_eq!(
            String::from_utf8(stripped.unwrap()).unwrap(),
            r"\newcommand{\logo}{\includegraphics{figs/red_square}}
\renewcommand{\logo}{\includegraphics{figs/red_square}}
\newcommand{\icon}[1]{\includegraphics[width=#1]{figs/red_square}}
\def\badge{\includegraphics{figs/red_square}}
"
        );
    }
}
