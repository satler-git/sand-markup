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
/// Renders the selected part(s) of a document as plain text or Markdown-formatted strings.
///
/// If the selector targets a specific named section, returns a single rendered string for that section.
/// Otherwise, returns a vector of rendered strings for all named sections in the document.
/// When `markdown` is true, section headers are formatted as Markdown headers.
///
/// # Parameters
/// - `markdown`: If true, formats output with Markdown-style section headers.
///
/// # Returns
/// A vector of rendered strings, each representing a section of the document.
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

/// Traverses the document AST according to the selector path and returns the targeted AST node and, if applicable, the index of the last path element in the document's names.
///
/// If the selector has a trailing dot or an empty path, returns the root AST and no target name index. Otherwise, follows the selector path through section-like nodes, matching by alias or numeric index, and returns the final AST node and the index of the last path element if found.
///
/// # Panics
///
/// Panics if the selector path is invalid, which should not occur if the selector has been validated beforehand.
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

/// Converts an AST node and its descendants to a plain text or Markdown-formatted string for a given name index and name.
///
/// If `markdown` is true, section nodes are rendered as Markdown headers with appropriate heading levels.
/// Otherwise, content is concatenated as plain text. Only content matching the specified name is included for nodes with named content.
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
    let re = regex::Regex::new(r"\\(.)").unwrap();
    re.replace_all(s, |caps: &regex::Captures| match &caps[1] {
        "n" => "\n".to_string(),
        "#" => "#".to_string(),
        "/" => "/".to_string(),
        "]" => "]".to_string(),
        "}" => "}".to_string(),
        "\\" => "\\".to_string(),
        other => format!("\\{other}"),
    })
    .into_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn trim() -> Result<(), Box<dyn std::error::Error>> {
        use super::trim;

        assert_eq!(
            trim(
                r#"
I'm thrilled!!
    It's because??
    I like you!
        "#
            ),
            "I'm thrilled!! It's because?? I like you!".to_string()
        );

        Ok(())
    }
}
