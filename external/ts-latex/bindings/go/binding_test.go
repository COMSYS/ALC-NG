package tree_sitter_latex_test

import (
	"testing"

	tree_sitter "github.com/tree-sitter/go-tree-sitter"
	tree_sitter_latex "github.com/latex-lsp/tree-sitter-latex/bindings/go"
)

func TestCanLoadGrammar(t *testing.T) {
	language := tree_sitter.NewLanguage(tree_sitter_latex.Language())
	if language == nil {
		t.Errorf("Error loading Latex grammar")
	}
}
