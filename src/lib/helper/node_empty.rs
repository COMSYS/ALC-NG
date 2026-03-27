use tree_sitter::Node;

use crate::cleaner::submission::parsed_file::GRAMMAR_NESTING;

pub fn is_empty(node: &Node) -> bool {
    match node.kind() {
        "line_comment" | "comment" | "whitespace" | "blockcomment" => true,
        "source_file" => {
            let mut i = 0;
            // Check all child nodes
            while let Some(child) = node.child(i) {
                if !is_empty(&child) {
                    return false;
                }

                i += 1;
            }

            true
        }
        x if GRAMMAR_NESTING.contains(&x) => {
            let mut i = 0;
            // Check all child nodes
            while let Some(child) = node.child(i) {
                if !is_empty(&child) {
                    return false;
                }

                i += 1;
            }

            true
        }
        "{" | "}" | "[" | "]" | "(" | ")" => true,
        _ => false,
    }
}
