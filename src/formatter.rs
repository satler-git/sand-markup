use pest::iterators::Pairs;

use crate::parser::{AST, Document, ParseError, Rule};

#[derive(Debug)]
pub struct Selector(pub AST);

impl TryFrom<(&Document, Pairs<'_, Rule>)> for Selector {
    type Error = Vec<ParseError>;

    fn try_from((doc, mut pairs): (&Document, Pairs<'_, Rule>)) -> Result<Self, Self::Error> {
        let pair = pairs.next().unwrap();

        let sel = crate::parser::parse_selector(pair.as_span().into(), pair);

        let errs = crate::parser::validate_non_local_selector(doc, &sel);

        if errs.is_empty() {
            Ok(Self(sel))
        } else {
            Err(errs)
        }
    }
}

// localでもDocumentの中のASTだけ差し替えるだけでいいはず
pub fn render_plain(doc: &Document, sel: &Selector, markdown: bool) -> Vec<String> {
    let (target_ast, target_name) = select(doc, sel);
    if let Some(target_name) = target_name {
        vec![
            to_plain(target_ast, (target_name, &doc.names[target_name]), markdown)
                .lines()
                .map(trim)
                .collect::<Vec<_>>()
                .join("\n"),
        ]
    } else {
        doc.names
            .iter()
            .enumerate()
            .map(|(index, name)| {
                to_plain(target_ast, (index, name), markdown)
                    .lines()
                    .map(trim)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .collect()
    }
}

fn select<'a>(doc: &'a Document, sel: &'a Selector) -> (&'a AST, Option<usize>) {
    if let Selector(AST {
        node: crate::parser::NodeKind::Selector {
            path, trailing_dot, ..
        },
        ..
    }) = sel
    {
        let (path, last) = if *trailing_dot || path.is_empty() {
            (path.as_ref(), None)
        } else {
            (
                &path[0..(path.len() - 1)],
                doc.names.iter().position(|t| t == path.last().unwrap()),
            )
        };

        let mut curr = &doc.ast;
        for pathi in path {
            if let Some((alias, children)) = curr.take_section_like() {
                if let Some(index) = alias.get(pathi) {
                    curr = &children[*index];
                } else if let Ok(index) = pathi.parse::<usize>() {
                    let children_without_sel: Vec<&AST> = children
                        .iter()
                        .filter(|p| !matches!(&p.node, crate::parser::NodeKind::Selector { .. }))
                        .collect();

                    curr = children_without_sel[index];
                } else {
                    panic!() // ここでselectorがvailedなのは保証されている
                }
            } else {
                break;
            }
        }

        (curr, last)
    } else {
        panic!()
    }
}

fn to_plain(ast: &AST, (name_i, name): (usize, &str), markdown: bool) -> String {
    let mut s = String::new();

    match &ast.node {
        crate::parser::NodeKind::Sen(v) => {
            s += &normalize(&trim(&v[name_i]));
        }
        crate::parser::NodeKind::All {
            all_or_names,
            content,
        } => {
            if all_or_names.is_none()
                || all_or_names.as_ref().map(|v| v.iter().any(|e| e == name)) == Some(true)
            {
                s += &normalize(&trim(content));
            }
        }
        crate::parser::NodeKind::Section {
            children,
            level,
            content,
            ..
        } => {
            if markdown {
                s += "\n\n";
                s += &"#".repeat(*level);
                s += " ";
                s += content;
                s += "\n\n";
            }

            for ci in children {
                s += " ";
                s += &to_plain(ci, (name_i, name), markdown);
            }
        }
        crate::parser::NodeKind::Top { children, .. } => {
            for ci in children {
                s += " ";
                s += &to_plain(ci, (name_i, name), markdown);
            }
        }
        _ => {}
    }

    s
}

fn trim(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize(s: &str) -> String {
    s.replace("\\#", "#")
        .replace("\\\\", "\\")
        .replace("\\/", "/")
        .replace("\\n", "\n")
        .replace("\\]", "]")
        .replace("\\}", "}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{AST, Document, NodeKind, NodeMeta, Span};
    use rustc_hash::FxHashMap;

    // Helper function to create a mock AST node with proper structure
    fn create_mock_ast(node: NodeKind) -> AST {
        AST {
            node,
            meta: NodeMeta {
                span: Span { start: 0, end: 0 },
                alias: None,
            },
        }
    }

    // Helper function to create a mock Document
    fn create_mock_document() -> Document {
        let ast = create_mock_ast(NodeKind::Top {
            children: vec![],
            aliases: FxHashMap::default(),
        });
        
        Document {
            ast,
            names: vec!["test".to_string(), "example".to_string()],
        }
    }

    #[test]
    fn trim_basic_whitespace() {
        assert_eq!(trim("  hello   world  "), "hello world");
    }

    #[test]
    fn trim_multiple_spaces() {
        assert_eq!(trim("a    b    c"), "a b c");
    }

    #[test]
    fn trim_newlines_and_tabs() {
        assert_eq!(trim("hello
	world
"), "hello world");
    }

    #[test]
    fn trim_empty_string() {
        assert_eq!(trim(""), "");
    }

    #[test]
    fn trim_only_whitespace() {
        assert_eq!(trim("   
	  "), "");
    }

    #[test]
    fn trim_multiline_with_indentation() {
        let input = r#"
I'm very happy!!
    It's because??
    I like you!
        "#;
        assert_eq!(trim(input), "I'm very happy!! It's because?? I like you!");
    }

    #[test]
    fn normalize_hash_escape() {
        assert_eq!(normalize("\\#header"), "#header");
    }

    #[test]
    fn normalize_backslash_escape() {
        assert_eq!(normalize("\\\\"), "\\");
    }

    #[test]
    fn normalize_slash_escape() {
        assert_eq!(normalize("\\/path"), "/path");
    }

    #[test]
    fn normalize_newline_escape() {
        assert_eq!(normalize("line1\\nline2"), "line1
line2");
    }

    #[test]
    fn normalize_bracket_escape() {
        assert_eq!(normalize("array\\]"), "array]");
    }

    #[test]
    fn normalize_brace_escape() {
        assert_eq!(normalize("object\\}"), "object}");
    }

    #[test]
    fn normalize_multiple_escapes() {
        assert_eq!(normalize("\\#\\\\\\n\\]"), "#\\
]");
    }

    #[test]
    fn normalize_no_escapes() {
        assert_eq!(normalize("normal text"), "normal text");
    }

    #[test]
    fn normalize_empty_string() {
        assert_eq!(normalize(""), "");
    }

    #[test]
    fn to_plain_sen_node() {
        let ast = create_mock_ast(NodeKind::Sen(vec![
            "Hello world".to_string(),
            "Bonjour monde".to_string(),
        ]));
        
        let result = to_plain(&ast, (0, "test"), false);
        assert_eq!(result, "Hello world");
        
        let result = to_plain(&ast, (1, "example"), false);
        assert_eq!(result, "Bonjour monde");
    }

    #[test]
    fn to_plain_sen_node_with_escapes() {
        let ast = create_mock_ast(NodeKind::Sen(vec![
            "Hello \\#world".to_string(),
        ]));
        
        let result = to_plain(&ast, (0, "test"), false);
        assert_eq!(result, "Hello #world");
    }

    #[test]
    fn to_plain_all_node_no_filter() {
        let ast = create_mock_ast(NodeKind::All {
            all_or_names: None,
            content: "Universal content".to_string(),
        });
        
        let result = to_plain(&ast, (0, "test"), false);
        assert_eq!(result, "Universal content");
    }

    #[test]
    fn to_plain_all_node_with_matching_name() {
        let ast = create_mock_ast(NodeKind::All {
            all_or_names: Some(vec!["test".to_string(), "other".to_string()]),
            content: "Filtered content".to_string(),
        });
        
        let result = to_plain(&ast, (0, "test"), false);
        assert_eq!(result, "Filtered content");
    }

    #[test]
    fn to_plain_all_node_with_non_matching_name() {
        let ast = create_mock_ast(NodeKind::All {
            all_or_names: Some(vec!["other".to_string(), "another".to_string()]),
            content: "Filtered content".to_string(),
        });
        
        let result = to_plain(&ast, (0, "test"), false);
        assert_eq!(result, "");
    }

    #[test]
    fn to_plain_section_node_without_markdown() {
        let child_ast = create_mock_ast(NodeKind::Sen(vec!["Child content".to_string()]));
        let ast = create_mock_ast(NodeKind::Section {
            children: vec![child_ast],
            level: 2,
            content: "Section Title".to_string(),
            aliases: FxHashMap::default(),
        });
        
        let result = to_plain(&ast, (0, "test"), false);
        assert_eq!(result, " Child content");
    }

    #[test]
    fn to_plain_section_node_with_markdown() {
        let child_ast = create_mock_ast(NodeKind::Sen(vec!["Child content".to_string()]));
        let ast = create_mock_ast(NodeKind::Section {
            children: vec![child_ast],
            level: 2,
            content: "Section Title".to_string(),
            aliases: FxHashMap::default(),
        });
        
        let result = to_plain(&ast, (0, "test"), true);
        assert_eq!(result, "

## Section Title

 Child content");
    }

    #[test]
    fn to_plain_section_node_different_levels() {
        let ast1 = create_mock_ast(NodeKind::Section {
            children: vec![],
            level: 1,
            content: "H1 Title".to_string(),
            aliases: FxHashMap::default(),
        });
        
        let ast3 = create_mock_ast(NodeKind::Section {
            children: vec![],
            level: 3,
            content: "H3 Title".to_string(),
            aliases: FxHashMap::default(),
        });
        
        let result1 = to_plain(&ast1, (0, "test"), true);
        assert_eq!(result1, "

# H1 Title

");
        
        let result3 = to_plain(&ast3, (0, "test"), true);
        assert_eq!(result3, "

### H3 Title

");
    }

    #[test]
    fn to_plain_top_node() {
        let child1 = create_mock_ast(NodeKind::Sen(vec!["First".to_string()]));
        let child2 = create_mock_ast(NodeKind::Sen(vec!["Second".to_string()]));
        let ast = create_mock_ast(NodeKind::Top {
            children: vec![child1, child2],
            aliases: FxHashMap::default(),
        });
        
        let result = to_plain(&ast, (0, "test"), false);
        assert_eq!(result, " First Second");
    }

    #[test]
    fn to_plain_unknown_node_type() {
        // Testing the default case (_) in the match statement
        let ast = create_mock_ast(NodeKind::Selector {
            local: false,
            path: vec!["test".to_string()],
            trailing_dot: false,
        });
        
        let result = to_plain(&ast, (0, "test"), false);
        assert_eq!(result, "");
    }

    #[test]
    fn to_plain_nested_sections() {
        let grandchild = create_mock_ast(NodeKind::Sen(vec!["Deep content".to_string()]));
        let child_section = create_mock_ast(NodeKind::Section {
            children: vec![grandchild],
            level: 3,
            content: "Subsection".to_string(),
            aliases: FxHashMap::default(),
        });
        let parent_section = create_mock_ast(NodeKind::Section {
            children: vec![child_section],
            level: 2,
            content: "Main Section".to_string(),
            aliases: FxHashMap::default(),
        });
        
        let result = to_plain(&parent_section, (0, "test"), true);
        assert!(result.contains("## Main Section"));
        assert!(result.contains("### Subsection"));
        assert!(result.contains("Deep content"));
    }

    #[test]
    fn selector_debug_trait() {
        let ast = create_mock_ast(NodeKind::Selector {
            local: false,
            path: vec!["test".to_string()],
            trailing_dot: false,
        });
        let selector = Selector(ast);
        
        // Just verify that Debug is implemented and doesn't panic
        let debug_output = format!("{:?}", selector);
        assert!(!debug_output.is_empty());
    }

    #[test]
    fn trim_unicode_whitespace() {
        // Test with various Unicode whitespace characters
        let input = "u{00A0}hellou{2000}worldu{2028}";
        let result = trim(input);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn normalize_edge_cases() {
        // Test edge cases where escape sequences might not be complete
        assert_eq!(normalize("\\"), "\\"); 
        assert_eq!(normalize("text\\"), "text\\"); 
        assert_eq!(normalize("\\#\\"), "#\\"); 
    }

    #[test]
    fn to_plain_with_whitespace_content() {
        let ast = create_mock_ast(NodeKind::Sen(vec![
            "  hello   world  ".to_string(),
        ]));
        
        let result = to_plain(&ast, (0, "test"), false);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn to_plain_empty_sen_vector() {
        let ast = create_mock_ast(NodeKind::Sen(vec![]));
        
        let result = std::panic::catch_unwind(|| {
            to_plain(&ast, (0, "test"), false)
        });
        
        assert!(result.is_err());
    }

    #[test]
    fn to_plain_index_boundary() {
        let ast = create_mock_ast(NodeKind::Sen(vec![
            "First".to_string(),
            "Second".to_string(),
        ]));
        
        assert_eq!(to_plain(&ast, (0, "test"), false), "First");
        assert_eq!(to_plain(&ast, (1, "test"), false), "Second");
        
        let result = std::panic::catch_unwind(|| {
            to_plain(&ast, (2, "test"), false)
        });
        assert!(result.is_err());
    }

    #[test]
    fn normalize_consecutive_escapes() {
        assert_eq!(normalize("\\#\\#\\n"), "##
");
        assert_eq!(normalize("\\\\\\\\"), "\\\\");
    }

    #[test]
    fn to_plain_all_node_empty_names_list() {
        let ast = create_mock_ast(NodeKind::All {
            all_or_names: Some(vec![]),
            content: "Should not appear".to_string(),
        });
        
        let result = to_plain(&ast, (0, "test"), false);
        assert_eq!(result, "");
    }

    #[test]
    fn trim_preserves_single_spaces() {
        assert_eq!(trim("a b c"), "a b c");
        assert_eq!(trim("hello world"), "hello world");
    }

    #[test]
    fn normalize_partial_escape_sequences() {
        assert_eq!(normalize("\\x"), "\\x"); 
        assert_eq!(normalize("\\"), "\\"); 
        assert_eq!(normalize("normal\\text"), "normal\\text"); 
    }

    #[test]
    fn select_function_with_selector() {
        let mut aliases = FxHashMap::default();
        aliases.insert("child1".to_string(), 0);
        
        let child_ast = create_mock_ast(NodeKind::Sen(vec!["Child content".to_string()]));
        let doc_ast = create_mock_ast(NodeKind::Top {
            children: vec![child_ast],
            aliases,
        });
        
        let doc = Document {
            ast: doc_ast,
            names: vec!["test".to_string()],
        };
        
        let selector_ast = create_mock_ast(NodeKind::Selector {
            local: false,
            path: vec!["child1".to_string(), "test".to_string()],
            trailing_dot: false,
        });
        let selector = Selector(selector_ast);
        
        let (result_ast, target_name) = select(&doc, &selector);
        assert!(matches!(result_ast.node, NodeKind::Sen(_)));
        assert_eq!(target_name, Some(0));
    }

    #[test]
    fn select_function_with_trailing_dot() {
        let child_ast = create_mock_ast(NodeKind::Sen(vec!["Child content".to_string()]));
        let doc_ast = create_mock_ast(NodeKind::Top {
            children: vec![child_ast],
            aliases: FxHashMap::default(),
        });
        
        let doc = Document {
            ast: doc_ast,
            names: vec!["test".to_string()],
        };
        
        let selector_ast = create_mock_ast(NodeKind::Selector {
            local: false,
            path: vec![],
            trailing_dot: true,
        });
        let selector = Selector(selector_ast);
        
        let (result_ast, target_name) = select(&doc, &selector);
        assert!(matches!(result_ast.node, NodeKind::Top { .. }));
        assert_eq!(target_name, None);
    }

    #[test]
    fn select_function_with_numeric_index() {
        let child1 = create_mock_ast(NodeKind::Sen(vec!["First".to_string()]));
        let child2 = create_mock_ast(NodeKind::Sen(vec!["Second".to_string()]));
        let section_ast = create_mock_ast(NodeKind::Section {
            children: vec![child1, child2],
            level: 1,
            content: "Section".to_string(),
            aliases: FxHashMap::default(),
        });
        
        let mut aliases = FxHashMap::default();
        aliases.insert("section".to_string(), 0);
        
        let doc_ast = create_mock_ast(NodeKind::Top {
            children: vec![section_ast],
            aliases,
        });
        
        let doc = Document {
            ast: doc_ast,
            names: vec!["test".to_string()],
        };
        
        let selector_ast = create_mock_ast(NodeKind::Selector {
            local: false,
            path: vec!["section".to_string(), "1".to_string(), "test".to_string()],
            trailing_dot: false,
        });
        let selector = Selector(selector_ast);
        
        let (result_ast, target_name) = select(&doc, &selector);
        assert!(matches!(result_ast.node, NodeKind::Sen(_)));
        assert_eq!(target_name, Some(0));
    }

    #[test]
    fn render_plain_with_selector_target() {
        let child_ast = create_mock_ast(NodeKind::Sen(vec!["Hello".to_string(), "Bonjour".to_string()]));
        let doc_ast = create_mock_ast(NodeKind::Top {
            children: vec![child_ast],
            aliases: FxHashMap::default(),
        });
        
        let doc = Document {
            ast: doc_ast,
            names: vec!["en".to_string(), "fr".to_string()],
        };
        
        let selector_ast = create_mock_ast(NodeKind::Selector {
            local: false,
            path: vec!["en".to_string()],
            trailing_dot: false,
        });
        let selector = Selector(selector_ast);
        
        let result = render_plain(&doc, &selector, false);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "Hello");
    }

    #[test]
    fn render_plain_with_all_names() {
        let child_ast = create_mock_ast(NodeKind::Sen(vec!["Hello".to_string(), "Bonjour".to_string()]));
        let doc_ast = create_mock_ast(NodeKind::Top {
            children: vec![child_ast],
            aliases: FxHashMap::default(),
        });
        
        let doc = Document {
            ast: doc_ast,
            names: vec!["en".to_string(), "fr".to_string()],
        };
        
        let selector_ast = create_mock_ast(NodeKind::Selector {
            local: false,
            path: vec![],
            trailing_dot: true,
        });
        let selector = Selector(selector_ast);
        
        let result = render_plain(&doc, &selector, false);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "Hello");
        assert_eq!(result[1], "Bonjour");
    }

    #[test]
    fn render_plain_with_markdown_mode() {
        let child_ast = create_mock_ast(NodeKind::Sen(vec!["Hello".to_string()]));
        let section_ast = create_mock_ast(NodeKind::Section {
            children: vec![child_ast],
            level: 2,
            content: "Greeting".to_string(),
            aliases: FxHashMap::default(),
        });
        let doc_ast = create_mock_ast(NodeKind::Top {
            children: vec![section_ast],
            aliases: FxHashMap::default(),
        });
        
        let doc = Document {
            ast: doc_ast,
            names: vec!["en".to_string()],
        };
        
        let selector_ast = create_mock_ast(NodeKind::Selector {
            local: false,
            path: vec!["en".to_string()],
            trailing_dot: false,
        });
        let selector = Selector(selector_ast);
        
        let result = render_plain(&doc, &selector, true);
        assert_eq!(result.len(), 1);
        assert!(result[0].contains("## Greeting"));
        assert!(result[0].contains("Hello"));
    }

    // Test complex normalization scenarios
    #[test]
    fn normalize_mixed_content() {
        let input = "Title: \\#Important\\nDescription: Some \\\\path\\nEnd";
        let expected = "Title: #Important
Description: Some \\path
End";
        assert_eq!(normalize(input), expected);
    }

    // Test trim behavior with mixed whitespace
    #[test]
    fn trim_complex_whitespace() {
        let input = "  	
  hello  
  world  	  ";
        assert_eq!(trim(input), "hello world");
    }

    // Test edge case where path is empty but trailing_dot is false
    #[test]
    fn select_empty_path_no_trailing_dot() {
        let doc_ast = create_mock_ast(NodeKind::Top {
            children: vec![],
            aliases: FxHashMap::default(),
        });
        
        let doc = Document {
            ast: doc_ast,
            names: vec!["test".to_string()],
        };
        
        let selector_ast = create_mock_ast(NodeKind::Selector {
            local: false,
            path: vec![],
            trailing_dot: false,
        });
        let selector = Selector(selector_ast);
        
        let (result_ast, target_name) = select(&doc, &selector);
        assert!(matches!(result_ast.node, NodeKind::Top { .. }));
        assert_eq!(target_name, None);
    }

    // Test that the original test still passes
    #[test]
    fn original_trim_test() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            trim(
                r#"
I'm very happy!!
    It's because??
    I like you!
        "#
            ),
            "I'm very happy!! It's because?? I like you!".to_string()
        );

        Ok(())
    }
}
