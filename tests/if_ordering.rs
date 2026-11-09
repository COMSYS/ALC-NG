mod common;

/// Conditionals are defined, set and used across files (`tests/if_ordering`):
/// - `config/settings.tex` defines all conditionals and sets their initial values,
/// - `main.tex` overrides `\ifextended` after loading the settings,
/// - `sections/results/setup.tex` sets `\ifshowtables`, which its parent `sections/results.tex` uses,
/// - `sections/disclaimer.tex` is loaded twice, with `\ifdraft` false and true.
#[cfg(test)]
mod tests {
    use crate::common::{is_pixel_identical, run_cleaner};

    use alc_ng::cleaner::config::CleanerConfig;
    use std::path::{Path, PathBuf};

    fn read(target: &Path, file: &str) -> String {
        std::fs::read_to_string(target.join(file)).unwrap()
    }

    #[test]
    fn conditionals_are_evaluated_across_files() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let (submission, target, result) = run_cleaner(
            "if_ordering_cleaned",
            &root.join("tests/if_ordering"),
            CleanerConfig {
                skip_watermark: true,
                ..Default::default()
            },
        );

        result.unwrap();

        // Defined and set in config/settings.tex, used in main.tex
        let main = read(&target, "main.tex");
        assert!(main.contains(r"\title{An Anonymous Submission}"), "{main}");
        assert!(!main.contains("A Named Submission"), "{main}");
        assert!(!main.contains(r"\ifanonymous"), "{main}");

        // Defined and set in config/settings.tex, used in a sibling file
        let intro = read(&target, "sections/intro.tex");
        assert!(intro.contains("submitted for blind review"), "{intro}");
        assert!(!intro.contains("Jane Doe"), "{intro}");
        assert!(!intro.contains(r"\ifanonymous"), "{intro}");

        // Set in a nested file, used by its parent after the \input
        let results = read(&target, "sections/results.tex");
        assert!(
            results.contains(r"\input{sections/results/table}"),
            "{results}"
        );
        assert!(!results.contains("Tables are omitted"), "{results}");
        assert!(!results.contains(r"\ifshowtables"), "{results}");

        // Set to true in config/settings.tex, but overridden in main.tex afterwards
        let table = read(&target, "sections/results/table.tex");
        assert!(!table.contains("C & D"), "{table}");
        assert!(!table.contains(r"\ifextended"), "{table}");

        let details = read(&target, "appendix/details.tex");
        assert!(details.contains("See the extended version"), "{details}");
        assert!(!details.contains("contains more details"), "{details}");
        assert!(!details.contains(r"\ifextended"), "{details}");

        // Loaded with both values: the conditional is kept, but both branches are cleaned
        let disclaimer = read(&target, "sections/disclaimer.tex");
        assert!(disclaimer.contains(r"\ifdraft"), "{disclaimer}");
        assert!(disclaimer.contains("This is a draft."), "{disclaimer}");
        assert!(
            disclaimer.contains("This is the final text."),
            "{disclaimer}"
        );

        // No comment survives in any file
        for file in [
            "main.tex",
            "config/settings.tex",
            "config/macros.tex",
            "sections/intro.tex",
            "sections/disclaimer.tex",
            "sections/results.tex",
            "sections/results/setup.tex",
            "sections/results/table.tex",
            "appendix/details.tex",
        ] {
            let content = read(&target, file);
            assert!(!content.contains("secret"), "{file}:\n{content}");
        }

        assert!(is_pixel_identical(&submission));
    }
}
