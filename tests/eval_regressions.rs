//! Constructs that broke papers of the arXiv evaluation.

use alc_ng::cleaner::config::CleanerConfig;
use alc_ng::cleaner::submission::parsed_file::ContentStripper;
use alc_ng::parsing::parse;
use std::sync::Arc;

fn clean_with(input: &[u8], force: bool) -> String {
    let tree = parse(input).unwrap();
    let config = CleanerConfig {
        force,
        ..Default::default()
    };
    let (stripped, _) =
        ContentStripper::clean(input, tree.root_node(), "test.tex", Arc::new(config)).unwrap();
    String::from_utf8(stripped.unwrap()).unwrap()
}

fn clean(input: &[u8]) -> String {
    clean_with(input, false)
}

/// Cleans content the grammar cannot parse completely, e.g. as `%` is not a comment.
fn clean_forced(input: &[u8]) -> String {
    clean_with(input, true)
}

#[test]
fn percent_with_other_catcode_in_group() {
    let input = br"{\catcode`\%=12\gdef\P@S@{%!}} % comment
\foo % comment
";
    assert_eq!(
        clean_forced(input),
        // The parser reads `%!}} % comment` as one comment, which is kept as a whole
        "{\\catcode`\\%=12\\gdef\\P@S@{%!}} % comment\n\\foo %\n"
    );
}

#[test]
fn percent_with_other_catcode_until_reset() {
    let input = br"\catcode`\%=12
\newcommand\pcnt{%}
\catcode`\%=14
\foo % comment
";
    assert_eq!(
        clean_forced(input),
        "\\catcode`\\%=12\n\\newcommand\\pcnt{%}\n\\catcode`\\%=14\n\\foo %\n"
    );
}

#[test]
fn deleted_conditional_before_end_of_group() {
    let input = br"\textbf{A
  \iffalse
    B
  \fi
}
";
    assert_eq!(clean(input), "\\textbf{A\n}\n");
}

#[test]
fn deleted_conditional_before_end_of_environment() {
    let input = br"\begin{proof}
Text.
\iffalse
a
\fi
\end{proof}
";
    assert_eq!(clean(input), "\\begin{proof}\nText.\n\\end{proof}\n");
}

#[test]
fn deleted_conditional_at_end_of_nested_section() {
    let input = br"\begin{proof}
\paragraph{Second.}
Text.
\iffalse
a
\fi
\end{proof}
";
    assert_eq!(
        clean(input),
        "\\begin{proof}\n\\paragraph{Second.}\nText.\n\\end{proof}\n"
    );
}

#[test]
fn iftrue_keeps_its_whole_branch() {
    let input = br"\iftrue
\address[1]{A}
\address[2]{B}
\else
\affiliation[1]{C}
\fi
x
";
    assert_eq!(clean(input), "\\address[1]{A}\n\\address[2]{B}\nx\n");
}

#[test]
fn comment_environment_keeps_earlier_lines() {
    let input = br"\end{theorem} % note
\begin{proof}
A \begin{comment} x
\end{comment}
\end{proof}
";
    assert_eq!(
        clean(input),
        "\\end{theorem} %\n\\begin{proof}\nA \\end{proof}\n"
    );
}

#[test]
fn iffalse_inside_definitions() {
    // The `\fi` of `\bco` is elsewhere
    let input = br"\def\bco{\iffalse}
\def\eco{\fi}
\newcommand{\review}{\iffalse} % comment
";
    assert_eq!(
        clean(input),
        "\\def\\bco{\\iffalse}\n\\def\\eco{\\fi}\n\\newcommand{\\review}{\\iffalse} %\n"
    );
}

#[test]
fn iffalse_balancing_braces() {
    let input = br"\def\x{\iffalse{\fi\ifnum0=1}\fi}
";
    assert_eq!(
        clean_forced(input),
        "\\def\\x{\\iffalse{\\fi\\ifnum0=1}\\fi}\n"
    );
}

#[test]
fn if_digit_depends_on_next_token() {
    // `\if0\blind` is true if `\blind` expands to 0, `\if1` followed by a space is false
    let input = br"\if0\blind
A
\fi
\if1
B
\else
C
\fi
\if0
D
\fi
";
    assert_eq!(
        clean(input),
        "\\if0\\blind\nA\n\\fi\n\\if1\nB\n\\else\nC\n\\fi\n"
    );
}

#[test]
fn newif_with_space() {
    let input = br"\newif \iffoo
\footrue
\iffoo A\fi
";
    // TeX skips the newline after `\fi`
    assert_eq!(clean(input), "\\newif \\iffoo\n\\footrue\nA");
}

#[test]
fn comment_package_kept_for_its_commands() {
    let input = br"\usepackage{comment}
\excludecomment{note}
";
    assert_eq!(
        clean(input),
        "\\usepackage{comment}\n\\excludecomment{note}\n"
    );
}

#[test]
fn internal_conditional_is_kept_without_makeatletter() {
    // arXiv 2107.03513: without `\makeatletter`, `\if@filesw` is `\if` comparing `@` and `f`,
    // which is false. Keeping only its branch exposed `\@auxout` (Improper \spacefactor).
    let input = br"\AtBeginDocument{%
    \immediate\write\bibnotes{@CONTROL{REVTEX41Control}}
     \if@filesw
     \immediate\write\@auxout{\string\citation{apsrev41Control}}% secret
    \fi
}%
";
    assert_eq!(
        clean(input),
        r"\AtBeginDocument{%
    \immediate\write\bibnotes{@CONTROL{REVTEX41Control}}
     \if@filesw
     \immediate\write\@auxout{\string\citation{apsrev41Control}}%
    \fi
}%
"
    );
}

#[test]
fn internal_conditionals_are_never_evaluated() {
    // arXiv 2404.11624: `\if@noskipsec` changes after each heading, which is not traced
    let input = b"\\makeatletter\n\\newif\\if@anon\\@anontrue\n\\if@anon A% secret\n\\else B\\fi\n\\if@noskipsec\\leavevmode\\fi\n";
    assert_eq!(
        clean(input),
        "\\makeatletter\n\\newif\\if@anon\\@anontrue\n\\if@anon A%\n\\else B\\fi\n\\if@noskipsec\\leavevmode\\fi\n"
    );
}

#[test]
fn comments_in_nested_error_node_are_removed() {
    // arXiv 1209.0897: `\left` and `\right` in different formulas (via macros) turn the whole
    // document body into an ERROR node, which was copied with all its comments
    let input = b"\\documentclass{article}\n\\begin{document}\n\\section{A}\nwhere $a\\right.$\\\\$\\left.-c$. % secret\n\\begin{equation}\nx % secret\n\\end{equation}\n\\end{document}\n";
    assert_eq!(
        clean_forced(input),
        "\\documentclass{article}\n\\begin{document}\n\\section{A}\nwhere $a\\right.$\\\\$\\left.-c$. %\n\\begin{equation}\nx %\n\\end{equation}\n\\end{document}"
    );
}

#[test]
fn tail_after_unparsed_end_document_is_removed() {
    // arXiv 2110.07300: the parser recovers from an error by splitting the document
    // environment, thus the content after `\end{document}` was kept
    let input = b"\\documentclass{article}\n\\begin{document}\n\\section{A}\nwhere $a\\right.$\\\\$\\left.-c$. % secret\n\\end{document}\n\\section{discarded}\nold % secret\n";
    assert_eq!(
        clean_forced(input),
        "\\documentclass{article}\n\\begin{document}\n\\section{A}\nwhere $a\\right.$\\\\$\\left.-c$. %\n\\end{document}"
    );
}

#[test]
fn end_document_in_block_comment_of_unparsed_document_is_kept() {
    let input = b"\\documentclass{article}\n\\begin{document}\nwhere $a\\right.$\\\\$\\left.-c$.\n\\iffalse\n\\end{document}\n\\fi\nkept\n\\end{document}\n";
    let cleaned = clean_forced(input);
    assert!(cleaned.contains("kept"), "{cleaned}");
}
