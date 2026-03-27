use std::sync::LazyLock;
use tree_sitter::{Parser, Query, QueryCursor, Tree};

pub static MAIN_QUERY: LazyLock<Query> = LazyLock::new(|| {
    Query::new(
        &tree_sitter_latex::language(),
        r#"[
        (class_include) @documentclass
        (generic_command
            command: (command_name) @cmd_name
            (#eq? @cmd_name "\\documentstyle")
            ) @documentstyle
        ]
        "#,
    )
    .unwrap()
});

pub fn parse(content: &[u8]) -> Option<Tree> {
    let mut parser = Parser::new();

    parser
        .set_language(&tree_sitter_latex::language())
        .expect("Error loading LaTeX grammar");

    parser.parse(content, None)
}

pub fn is_main_tex(tree: &Tree, content: &[u8]) -> bool {
    use tree_sitter::StreamingIterator;

    let mut cursor = QueryCursor::new();
    let matches = cursor.matches(&MAIN_QUERY, tree.root_node(), content);

    matches.count() > 0
}
