#[cfg(test)]
mod tests {
    use alc_ng::cleaner::config::CleanerConfig;
    use alc_ng::cleaner::submission::parsed_file::ContentStripper;
    use alc_ng::parsing::parse;
    use std::sync::Arc;

    fn clean(input: &[u8]) -> String {
        let tree = parse(input).unwrap();
        let (stripped, _) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        String::from_utf8(stripped.unwrap()).unwrap()
    }

    #[test]
    fn text_after_end_document_is_removed() {
        assert_eq!(
            clean(b"\\begin{document}\nText\n\\end{document}\nTail\n"),
            "\\begin{document}\nText\n\\end{document}"
        );
    }

    /// The parser pairs `\begin{document}` with `\end{minipage}` after the stray brace
    /// (minimized from arXiv 0710.4345). The rest of the document must not be treated as tail.
    #[test]
    fn document_paired_with_other_end_keeps_rest() {
        let cleaned =
            clean(b"\\begin{document}\nText\n}\\end{minipage}\nMore text\n\\end{document}\n");

        assert!(cleaned.contains("More text"));
        assert!(cleaned.contains("\\end{document}"));
    }
}
