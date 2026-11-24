mod common;

/// Conditionals that are set by commands or used in definitions (`tests/if_setters`).
/// The probe traces each assignment, thus a conditional in a definition is evaluated if its
/// value is the same whenever the defined command may be used.
#[cfg(test)]
mod tests {
    use crate::common::{is_pixel_identical, run_cleaner};

    use alc_ng::cleaner::config::CleanerConfig;
    use std::path::PathBuf;

    #[test]
    fn conditionals_set_by_commands_and_in_definitions() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let (submission, target, result) = run_cleaner(
            "if_setters_cleaned",
            &root.join("tests/if_setters"),
            CleanerConfig {
                skip_watermark: true,
                ..Default::default()
            },
        );

        result.unwrap();
        let main = std::fs::read_to_string(target.join("main.tex")).unwrap();

        // Never changed after the definition: the unused branch is removed, the command is
        // empty and its invocation removed
        assert!(!main.contains(r"\ifdraft\textbf"), "{main}");
        assert!(!main.contains("a secret note"), "{main}");

        // Constant in the document, where the command is used
        assert!(main.contains("Jane Doe"), "{main}");
        assert!(!main.contains("Anonymous authors"), "{main}");

        // Set by `\finalcopy` of the template, not only when the template returned
        assert!(main.contains(r"\pagestyle{empty}"), "{main}");
        assert!(!main.contains(r"\iffinal"), "{main}");

        // Toggled in the document
        assert!(main.contains(r"\def\sep{\ifprev, \fi}"), "{main}");

        // Expanded in the preamble with another value than in the document
        assert!(
            main.contains(r"\newcommand{\early}{\ifearly early\else late\fi}"),
            "{main}"
        );

        assert!(is_pixel_identical(&submission));
    }
}
