#[cfg(test)]
mod tests {
    use alc_ng::cleaner::config::CleanerConfig;
    use alc_ng::cleaner::submission::parsed_file::ContentStripper;
    use alc_ng::parsing::parse;
    use std::sync::Arc;

    fn clean(input: &[u8]) -> String {
        let tree = parse(input).unwrap();
        let (stripped, deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        assert!(deletion_stats.grammar_errors.is_empty());
        String::from_utf8(stripped.unwrap()).unwrap()
    }

    #[test]
    fn empty_def_with_delimited_parameter_is_kept() {
        // The text up to the delimiter is not part of the invocation,
        // so deleting the invocation would reveal the hidden text.
        let input = br#"\def\ignore#1\endignore{}
Visible. \ignore hidden draft text \endignore Visible again.
"#;

        assert_eq!(clean(input), String::from_utf8_lossy(input));
    }

    #[test]
    fn empty_def_with_delimiter_after_parameter_is_kept() {
        let input = br#"\def\skipdot#1.{}
A \skipdot secret. B
"#;

        assert_eq!(clean(input), String::from_utf8_lossy(input));
    }

    #[test]
    fn empty_def_with_undelimited_parameters_is_deleted() {
        let input = br#"\def\drop#1#2{}
A \drop{gone}{too} B
"#;

        assert_eq!(clean(input), "A {} B\n");
    }
}
