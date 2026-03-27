fn main() {
    let src_dir = std::path::Path::new("src");

    let grammar_path = std::path::Path::new(".");
    let grammar_file = grammar_path.join("grammar.js");

    tree_sitter_generate::generate_parser_in_directory(
        &grammar_path,
        Some(&src_dir),
        Some(&grammar_file),
        tree_sitter_generate::ABI_VERSION_MAX,
        None,
        None,
        true,
        tree_sitter_generate::OptLevel::default(),
    )
    .expect("Failed to generate parser");
    println!("cargo:rerun-if-changed=grammar.js");

    let mut c_config = cc::Build::new();
    c_config.cargo_warnings(false);
    c_config.std("c11").include(src_dir);

    #[cfg(target_env = "msvc")]
    c_config.flag("-utf-8");

    let parser_path = src_dir.join("parser.c");
    c_config.file(&parser_path);

    let scanner_path = src_dir.join("scanner.c");
    c_config.file(&scanner_path);

    c_config.compile("tree-sitter-latex");
}
