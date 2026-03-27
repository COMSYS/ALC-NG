mod test {
    use alc_ng::helper::is_empty;
    use alc_ng::parsing::parse;

    #[test]
    fn test_empty_curly_group() {
        let input = br#"
            {
            %

            %
            }"#;
        let tree = parse(input).unwrap();
        let empty = is_empty(&tree.root_node());

        assert!(empty);
    }

    #[test]
    fn test_non_empty_curly_group() {
        let input = br#"
            {
            %
            some content
            %
            }"#;
        let tree = parse(input).unwrap();
        let empty = is_empty(&tree.root_node());

        assert!(!empty);
    }

    #[test]
    fn test_nested_empty_groups() {
        let input = br#"
            {
            %
            {
            %
            %
            }
            %
            }"#;
        let tree = parse(input).unwrap();
        let empty = is_empty(&tree.root_node());

        assert!(empty);
    }

    #[test]
    fn test_nested_non_empty_groups() {
        let input = br#"
            {
            %
            {
            %
            content
            %
            }
            %
            }"#;
        let tree = parse(input).unwrap();
        let empty = is_empty(&tree.root_node());

        assert!(!empty);
    }

    #[test]
    fn test_empty_with_whitespace() {
        let input = br#"
            {
            %

            %
            }"#;
        let tree = parse(input).unwrap();
        let empty = is_empty(&tree.root_node());

        assert!(empty);
    }

    #[test]
    fn test_non_empty_with_comments() {
        let input = br#"
            {
            %
            % comment
            %
            }"#;
        let tree = parse(input).unwrap();
        let empty = is_empty(&tree.root_node());

        assert!(empty);
    }
}
