#[cfg(test)]
mod tests {
    use alc_ng::cleaner::config::CleanerConfig;
    use alc_ng::cleaner::submission::parsed_file::ContentStripper;
    use alc_ng::helper::ContainsByteSlice;
    use alc_ng::parsing::parse;
    use std::sync::Arc;

    #[test]
    fn ifcsname() {
        let input = br#"
        \newcommand{\V}[1]{%
            \ifcsname var-#1\endcsname%
            %\pdfmarkupcomment[markup=Underline,color=purple]{{\color{purple}{\csname var-#1\endcsname}}}{{#1: \csname desc-#1\endcsname}}%
            {\color{purple}{\csname var-#1\endcsname}}%
            \else%
            {\color{purple}{\small{\{#1\}}}}%
            \fi%
            }
"#;
        let tree = parse(input).unwrap();
        let (stripped, _deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        let stripped = stripped.unwrap();
        assert!(stripped.contains_slice(br"\ifcsname var-#1\endcsname"));
        assert!(!stripped.contains_slice(br"pdfmarkupcomment"));
        assert!(_deletion_stats.grammar_errors.len() == 0);
    }
}
