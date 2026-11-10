/// Cleaning with the values of custom conditionals traced while compiling (`IfTrace`).
#[cfg(test)]
mod tests {
    use alc_ng::cleaner::config::CleanerConfig;
    use alc_ng::cleaner::submission::comment_commands::CommentCommands;
    use alc_ng::cleaner::submission::if_trace::{FileIfState, IfValue, IfValues};
    use alc_ng::cleaner::submission::parsed_file::ContentStripper;
    use alc_ng::parsing::parse;
    use std::sync::Arc;

    fn values(values: &[(&str, IfValue)]) -> IfValues {
        values
            .iter()
            .map(|(name, value)| (name.as_bytes().to_vec(), *value))
            .collect()
    }

    fn clean(input: &[u8], if_state: Option<FileIfState>) -> anyhow::Result<String> {
        let tree = parse(input).unwrap();
        let (stripped, _) = ContentStripper::clean_with_comment_commands(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
            None,
            Arc::new(CommentCommands::default()),
            if_state,
        )?;

        Ok(String::from_utf8(stripped.unwrap()).unwrap())
    }

    #[test]
    fn values_at_the_start_of_the_file_are_used() {
        let input = b"\\ifdraft\nDraft % comment\n\\else\nFinal\n\\fi\n";
        let state = |value| FileIfState {
            entry: values(&[("draft", IfValue::Known(value))]),
            returns: Vec::new(),
        };

        assert!(clean(input, None).is_err());
        // The comment in front of `\else` ends the branch and is removed with its newline,
        // which TeX reads the same way
        assert_eq!(clean(input, Some(state(true))).unwrap(), "Draft ");
        assert_eq!(clean(input, Some(state(false))).unwrap(), "Final\n");
    }

    #[test]
    fn values_after_a_loaded_file_are_used_on_the_next_lines() {
        let input =
            b"\\ifdraft\nA\n\\else\nB\n\\fi\n\\input{settings}\n\\ifdraft\nC\n\\else\nD\n\\fi\n";
        let state = FileIfState {
            entry: values(&[("draft", IfValue::Known(false))]),
            returns: vec![(2, values(&[("draft", IfValue::Known(true))]))],
        };

        let cleaned = clean(input, Some(state)).unwrap();
        assert_eq!(cleaned, "B\n\\input{settings}\nC\n");
    }

    #[test]
    fn values_changed_on_the_same_line_are_unknown() {
        let input = b"\\input{settings}\\ifdraft A\\else B\\fi\n";
        let state = FileIfState {
            entry: values(&[("draft", IfValue::Known(false))]),
            returns: vec![(1, values(&[("draft", IfValue::Known(true))]))],
        };

        assert_eq!(
            clean(input, Some(state)).unwrap(),
            String::from_utf8_lossy(input)
        );
    }

    #[test]
    fn unknown_values_keep_both_branches_cleaned() {
        let input = b"\\ifdraft\nDraft % comment one\n\\else\nFinal % comment two\n\\fi\n";
        let state = FileIfState {
            entry: values(&[("draft", IfValue::Unknown)]),
            returns: Vec::new(),
        };

        assert_eq!(
            clean(input, Some(state)).unwrap(),
            "\\ifdraft\nDraft %\n\\else\nFinal %\n\\fi\n"
        );
    }

    #[test]
    fn assignments_are_used_afterwards() {
        let input = b"\\drafttrue\n\\ifdraft\nA\n\\else\nB\n\\fi\n";
        let state = FileIfState {
            entry: values(&[("draft", IfValue::Unknown)]),
            returns: Vec::new(),
        };

        assert_eq!(clean(input, Some(state)).unwrap(), "\\drafttrue\nA\n");
    }

    /// The body of a definition is executed when the command is used, not where it is defined.
    #[test]
    fn assignments_in_definitions_make_the_value_unknown() {
        let input =
            b"\\newif\\ifdraft\n\\newcommand{\\setdraft}{\\drafttrue}\n\\ifdraft A\\else B\\fi\n";

        assert_eq!(clean(input, None).unwrap(), String::from_utf8_lossy(input));
    }

    /// An assignment in a branch of a conditional whose value is not known may not be executed.
    #[test]
    fn assignments_in_kept_branches_make_the_value_unknown() {
        let input = b"\\newif\\ifdraft\n\\ifdefined\\foo\\drafttrue\\fi\n\\ifdraft A\\else B\\fi\n";

        assert_eq!(clean(input, None).unwrap(), String::from_utf8_lossy(input));
    }
}
