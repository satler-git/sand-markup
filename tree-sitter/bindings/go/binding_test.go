package tree_sitter_sand_test

import (
	"testing"

	tree_sitter "github.com/tree-sitter/go-tree-sitter"
	tree_sitter_sand "github.com/satler-git/sand-markup/bindings/go"
)

func TestCanLoadGrammar(t *testing.T) {
	language := tree_sitter.NewLanguage(tree_sitter_sand.Language())
	if language == nil {
		t.Errorf("Error loading Sand grammar")
	}
}
