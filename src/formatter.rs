use pest::iterators::Pairs;

use crate::parser::{AST, Document, ParseError, Rule};

#[derive(Debug)]
pub struct Selector(AST);

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
pub fn render_plain(doc: &Document, sel: &Selector) -> Vec<String> {
    let (target_ast, target_name) = select(doc, sel);
    if let Some(target_name) = target_name {
        vec![
            to_plain(target_ast, (target_name, &doc.names[target_name]))
                .lines()
                .map(|s| trim(s))
                .collect::<Vec<_>>()
                .join("\n"),
        ]
    } else {
        doc.names
            .iter()
            .enumerate()
            .map(|(index, name)| {
                to_plain(target_ast, (index, name))
                    .lines()
                    .map(|s| trim(s))
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
            let (alias, children) = curr.take_section_like().unwrap();
            let children_without_sel: Vec<&AST> = children
                .iter()
                .filter(|p| !matches!(&p.node, crate::parser::NodeKind::Selector { .. }))
                .collect();

            if let Some(index) = alias.get(pathi) {
                curr = children_without_sel[*index];
            } else if let Ok(index) = pathi.parse::<usize>() {
                curr = children_without_sel[index];
            } else {
                panic!() // ここでselectorがvailedなのは保証されている
            }
        }

        (curr, last)
    } else {
        panic!()
    }
}

fn to_plain(ast: &AST, (name_i, name): (usize, &str)) -> String {
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
            // TODO: markdownではsection動作だけ変えればいい？
            children,
            ..
        }
        | crate::parser::NodeKind::Top { children, .. } => {
            for ci in children {
                s += " ";
                s += &to_plain(ci, (name_i, name));
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
    #[test]
    fn trim() -> Result<(), Box<dyn std::error::Error>> {
        use super::trim;

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
