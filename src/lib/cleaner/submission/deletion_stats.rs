use log::info;

#[derive(Debug, Clone, Default)]
pub struct DeletionStats {
    pub comments: Vec<Vec<u8>>,
    pub if_evaluated: Vec<Vec<u8>>,
    pub if_evaluated_empty: Vec<Vec<u8>>,
    pub suffix_comment: Vec<Vec<u8>>,
    pub line_comment: Vec<Vec<u8>>,
    pub out_of_bounds: Vec<Vec<u8>>,
    pub comment_definition_deleted: Vec<Vec<u8>>,
    pub comment_invocation_deleted: Vec<Vec<u8>>,
    pub comment_package_include_deleted: Vec<Vec<u8>>,
    pub block_comment_deleted: Vec<Vec<u8>>,
    pub bib_deleted: Vec<Vec<u8>>,
    pub files_deleted: Vec<String>,
}

impl DeletionStats {
    pub fn merge(&mut self, other: DeletionStats) {
        self.comments.extend(other.comments);
        self.if_evaluated.extend(other.if_evaluated);
        self.if_evaluated_empty.extend(other.if_evaluated_empty);
        self.suffix_comment.extend(other.suffix_comment);
        self.line_comment.extend(other.line_comment);
        self.out_of_bounds.extend(other.out_of_bounds);
        self.comment_definition_deleted
            .extend(other.comment_definition_deleted);
        self.comment_invocation_deleted
            .extend(other.comment_invocation_deleted);
        self.comment_package_include_deleted
            .extend(other.comment_package_include_deleted);
        self.block_comment_deleted
            .extend(other.block_comment_deleted);
        self.bib_deleted.extend(other.bib_deleted);
        self.files_deleted.extend(other.files_deleted);
    }

    pub fn pretty_print(&self, verbose: bool) {
        info!("=== Deletion Statistics ===");

        self.print_vec_preview("comments", &self.comments, 3, verbose);
        self.print_vec_preview("if_evaluated", &self.if_evaluated, 3, verbose);
        self.print_vec_preview("if_evaluated_empty", &self.if_evaluated_empty, 3, verbose);
        self.print_vec_preview("suffix_comment", &self.suffix_comment, 3, verbose);
        self.print_vec_preview("line_comment", &self.line_comment, 3, verbose);
        self.print_vec_preview("out_of_bounds", &self.out_of_bounds, 3, verbose);
        self.print_vec_preview(
            "comment_definition_deleted",
            &self.comment_definition_deleted,
            3,
            verbose,
        );
        self.print_vec_preview(
            "comment_invocation_deleted",
            &self.comment_invocation_deleted,
            3,
            verbose,
        );
        self.print_vec_preview(
            "comment_package_include_deleted",
            &self.comment_package_include_deleted,
            3,
            verbose,
        );
        self.print_vec_preview(
            "block_comment_deleted",
            &self.block_comment_deleted,
            3,
            verbose,
        );
        self.print_vec_preview("bib_deleted", &self.bib_deleted, 3, verbose);
        self.print_string_vec_preview("files_deleted", &self.files_deleted, 3, verbose);

        info!("=== End Deletion Statistics ===");
    }

    fn print_vec_preview(&self, name: &str, vec: &[Vec<u8>], max_preview: usize, verbose: bool) {
        let count = vec.len();
        info!("{}: {} items", name, count);

        if verbose {
            for (i, item) in vec.iter().enumerate() {
                let preview_str = String::from_utf8_lossy(item);
                info!("  [{}] {}", i + 1, preview_str);
            }
        } else {
            let preview_count = std::cmp::min(vec.len(), max_preview);
            for (i, item) in vec.iter().take(preview_count).enumerate() {
                let preview_str = String::from_utf8_lossy(item);
                let preview = preview_str
                    .lines()
                    .next()
                    .unwrap_or("")
                    .chars()
                    .take(50)
                    .collect::<String>();
                info!("  [{}] {}", i + 1, preview);
            }
        }
    }

    fn print_string_vec_preview(
        &self,
        name: &str,
        vec: &[String],
        max_preview: usize,
        verbose: bool,
    ) {
        let count = vec.len();
        info!("{}: {} items", name, count);

        if verbose {
            for (i, item) in vec.iter().enumerate() {
                info!("  [{}] {}", i + 1, item);
            }
        } else {
            let preview_count = std::cmp::min(vec.len(), max_preview);
            for (i, item) in vec.iter().take(preview_count).enumerate() {
                let preview = item.chars().take(50).collect::<String>();
                info!("  [{}] {}", i + 1, preview);
            }
        }
    }
}
