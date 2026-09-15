#[cfg(test)]
mod tests {
    use alc_ng::cleaner::config::CleanerConfig;
    use alc_ng::cleaner::submission::parsed_file::ContentStripper;
    use alc_ng::parsing::parse;
    use std::sync::Arc;

    /// Clean the input and return the output as a UTF-8 string.
    fn clean(input: &[u8]) -> Option<String> {
        let tree = parse(input).unwrap();
        let (stripped, _stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();
        stripped.map(|b| String::from_utf8_lossy(&b).into_owned())
    }

    #[test]
    fn keep_all_is_byte_identical() {
        let input = br#"% !ALC-NG keep-all
% a secret comment
\documentclass{article}
\begin{document}
Hello % trailing secret
\end{document}
trailing junk"#;
        let out = clean(input).unwrap();
        assert_eq!(out, String::from_utf8_lossy(input));
    }

    #[test]
    fn keep_comments_preserves_line_and_suffix_comments() {
        let input = br#"% !ALC-NG keep-comments
% another secret
\documentclass{article}
\begin{document}
Hello % inline secret
World
\end{document}"#;
        let out = clean(input).unwrap();
        assert!(out.contains("another secret"));
        assert!(out.contains("inline secret"));
        assert!(out.contains("Hello"));
        assert!(out.contains("World"));
        assert!(out.contains(r"\documentclass{article}"));
    }

    #[test]
    fn keep_comments_not_first_comment() {
        let input = br#"% header note
% !ALC-NG keep-comments
% another secret
\documentclass{article}
\begin{document}
Hello
\end{document}"#;
        let out = clean(input).unwrap();
        assert!(out.contains("header note"));
        assert!(out.contains("another secret"));
    }

    #[test]
    fn keep_comments_preserves_comment_env_and_package() {
        let input = br#"% !ALC-NG keep-comments
\documentclass{article}
\usepackage{comment}
\begin{document}
\begin{comment}
hidden secret
\end{comment}
World
\end{document}"#;
        let out = clean(input).unwrap();
        assert!(out.contains("hidden secret"));
        assert!(out.contains(r"\usepackage{comment}"));
        assert!(out.contains("World"));
    }

    #[test]
    fn keep_comments_preserves_custom_comment_command() {
        let input = br#"% !ALC-NG keep-comments
\documentclass{article}
\newcommand{\cmt}[1]{}
\begin{document}
Hello \cmt{secret comment}
World
\end{document}"#;
        let out = clean(input).unwrap();
        assert!(out.contains("secret comment"));
        assert!(out.contains(r"\newcommand{\cmt}[1]{}"));
        assert!(out.contains("Hello"));
    }

    #[test]
    fn directive_line_kept_even_when_comments_stripped() {
        // Only keep-ifs is set: regular comments are stripped, but the
        // directive line itself is always preserved.
        let input = br#"% !ALC-NG keep-ifs
% a regular secret
\documentclass{article}
\begin{document}
Hello
\end{document}"#;
        let out = clean(input).unwrap();
        assert!(out.contains("!ALC-NG keep-ifs"));
        assert!(!out.contains("a regular secret"));
    }

    #[test]
    fn keep_ifs_preserves_if_block() {
        let input = br#"% !ALC-NG keep-ifs
\documentclass{article}
\begin{document}
\iffalse
hidden if content
\fi
Visible
\end{document}"#;
        let out = clean(input).unwrap();
        assert!(out.contains("hidden if content"));
        assert!(out.contains(r"\iffalse"));
        assert!(out.contains(r"\fi"));
        assert!(out.contains("Visible"));
    }

    #[test]
    fn keep_tail_preserves_content_after_end_document() {
        let input = br#"% !ALC-NG keep-tail
\documentclass{article}
\begin{document}
Hello
\end{document}
trailing secret"#;
        let out = clean(input).unwrap();
        assert!(out.contains("trailing secret"));
        assert!(out.contains("Hello"));
    }

    #[test]
    fn verbatim_directive_is_ignored() {
        // A keep-all inside verbatim must NOT disable cleaning. The real
        // line comment outside is stripped; the verbatim body is preserved.
        let input = br#"\documentclass{article}
\begin{verbatim}
% !ALC-NG keep-all
\end{verbatim}
% real secret comment
\begin{document}
Hello
\end{document}"#;
        let out = clean(input).unwrap();
        assert!(out.contains("% !ALC-NG keep-all")); // preserved as verbatim body
        assert!(!out.contains("real secret comment")); // real comment is stripped
    }

    #[test]
    fn no_directive_still_cleans() {
        let input = br#"\documentclass{article}
% a secret
\begin{document}
Hello
\end{document}"#;
        let out = clean(input).unwrap();
        assert!(!out.contains("a secret"));
        assert!(out.contains("Hello"));
    }

    #[test]
    fn listing_directive_is_ignored() {
        let input = br#"\documentclass{article}
\begin{lstlisting}
% !ALC-NG keep-all
\end{lstlisting}
% real secret comment
\begin{document}
Hello
\end{document}"#;
        let out = clean(input).unwrap();
        assert!(out.contains("% !ALC-NG keep-all")); // preserved as listing body
        assert!(!out.contains("real secret comment")); // real comment is stripped
    }
}
