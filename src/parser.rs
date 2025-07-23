use pest::iterators::Pairs;
use pest_derive::Parser;
use rustc_hash::{FxHashMap, FxHashSet};

#[derive(Parser)]
#[grammar = "sand.pest"]
pub struct SandParser;

#[derive(Debug)]
pub struct Document {
    pub names: Vec<String>,
    pub ast: AST,
}

#[derive(Debug, Clone)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl From<pest::Span<'_>> for Span {
    fn from(value: pest::Span) -> Self {
        Self {
            start: value.start(),
            end: value.end(),
        }
    }
}

use thiserror::Error;
#[derive(Error, Debug)]
pub enum ParseError {
    #[error("names are defined more than once")]
    MultipleNameDefine(Span),
    #[error("the same names are defined more than once: {0}")]
    DuplicateNames(String, Span),
    #[error("aliases are duplicated: {0}")]
    DuplicateAlias(String, Span),
    #[error("aliases and names are conflicted: {0}")]
    AliasConflictWithNames(String, Span),
    #[error("names are not defined")]
    MissingNames,
}

use codespan_reporting::diagnostic::{Diagnostic, Label};

pub fn convert_parse_error(file_id: usize, err: &ParseError) -> Diagnostic<usize> {
    match err {
        ParseError::MultipleNameDefine(span) => Diagnostic::error()
            .with_message("names are defined more than once")
            .with_labels(vec![
                Label::primary(file_id, span.start..span.end)
                    .with_message("this is a repeated definition"),
            ]),
        ParseError::DuplicateNames(name, span) => Diagnostic::error()
            .with_message(format!("duplicate name: `{name}`"))
            .with_labels(vec![
                Label::primary(file_id, span.start..span.end).with_message("duplicate name here"),
            ]),
        ParseError::DuplicateAlias(name, span) => Diagnostic::error()
            .with_message(format!("duplicate alias: `{name}`"))
            .with_labels(vec![
                Label::primary(file_id, span.start..span.end).with_message("duplicate alias here"),
            ]),
        ParseError::AliasConflictWithNames(name, span) => Diagnostic::error()
            .with_message(format!("alias `{name}` conflicts with a name"))
            .with_labels(vec![
                Label::primary(file_id, span.start..span.end)
                    .with_message("this alias conflicts with a name"),
            ]),
        ParseError::MissingNames => Diagnostic::error().with_message("names are not defined"),
    }
}

pub fn convert_pest_error(file_id: usize, error: pest::error::Error<Rule>) -> Diagnostic<usize> {
    use pest::error::ErrorVariant;

    let span = {
        let (start, end) = match error.location {
            pest::error::InputLocation::Pos(pos) => (pos, pos + 1),
            pest::error::InputLocation::Span((s, e)) => (s, e),
        };
        Span { start, end }
    };

    match error.variant {
        ErrorVariant::ParsingError {
            positives,
            negatives,
        } => {
            let mut msg = String::from("failed to parse input");
            if !positives.is_empty() {
                msg += &format!(", expected: {positives:?}");
            }
            if !negatives.is_empty() {
                msg += &format!(", not: {negatives:?}");
            }

            Diagnostic::error()
                .with_message(msg)
                .with_labels(vec![Label::primary(file_id, span.start..span.end)])
        }
        ErrorVariant::CustomError { message } => Diagnostic::error()
            .with_message(message)
            .with_labels(vec![Label::primary(file_id, span.start..span.end)]),
    }
}

// TODO: validateでエラーをまとめて出す
impl TryFrom<Pairs<'_, Rule>> for Document {
    type Error = Vec<ParseError>;

    fn try_from(mut pairs: Pairs<'_, Rule>) -> Result<Self, Vec<ParseError>> {
        let mut ast = vec![AST {
            node: NodeKind::Top {
                aliases: FxHashMap::default(),
                children: vec![],
            },
            meta: NodeMeta {
                alias: None,
                span: None,
            },
        }];
        let mut names: Option<Vec<String>> = None;

        let mut errs = vec![];

        let root = pairs.next().unwrap();

        for pair in root.into_inner() {
            let span: Span = pair.as_span().into();

            let mut to_push_at_last = None;

            match pair.as_rule() {
                Rule::PartName => {
                    if names.is_some() {
                        errs.push(ParseError::MultipleNameDefine(span.clone())); // TODO: これだとdupのと一貫性がないかも
                    }
                    let ident_list_pair = pair.into_inner().next().unwrap();

                    let raw_names: Vec<String> = ident_list_pair
                        .into_inner()
                        .filter(|p| p.as_rule() == Rule::Ident)
                        .map(|p| p.as_str().to_string())
                        .collect();

                    let mut seen = FxHashSet::default();
                    for name in &raw_names {
                        if !seen.insert(name.clone()) {
                            errs.push(ParseError::DuplicateNames(name.clone(), span.clone()));
                        }
                    }

                    names = Some(raw_names);
                }
                Rule::Section => {
                    let mut inner = pair.into_inner();

                    let alias = take_alias(&mut inner);

                    let hashes = inner.next().unwrap().as_str();
                    let level = hashes.chars().count();

                    let content = inner.next().unwrap().as_str().to_string();

                    let mut top_level =
                        { (ast.last_mut().unwrap()).take_mut_section_like().unwrap().0 };

                    while ast.len() > 1 && top_level >= level {
                        let top = ast.pop().unwrap();

                        if let Some(last) = ast.last_mut() {
                            let (new_top_level, a, v) = last.take_mut_section_like().unwrap();
                            top_level = new_top_level;

                            if let Some(ref alias) = top.meta.alias {
                                check_alias_conflict(
                                    alias,
                                    a,
                                    v,
                                    v.len(),
                                    top.get_span().unwrap(),
                                    &mut errs,
                                );
                            }

                            v.push(top);
                        }
                    }

                    ast.push(AST {
                        meta: NodeMeta {
                            span: Some(span),
                            alias,
                        },
                        node: NodeKind::Section {
                            level,
                            content,
                            aliases: FxHashMap::default(),
                            children: vec![],
                        },
                    });
                }
                Rule::ApplyAll => {
                    let mut inner = pair.into_inner();

                    let alias = take_alias(&mut inner);
                    let p = inner.next().unwrap();
                    let elements = match p.as_rule() {
                        Rule::string => (None, p.as_str().into()),
                        Rule::Idents => (
                            Some(
                                p.into_inner()
                                    .next()
                                    .unwrap()
                                    .into_inner()
                                    .filter(|p| p.as_rule() == Rule::Ident)
                                    .map(|p| p.as_str().to_string())
                                    .collect(),
                            ),
                            inner.next().unwrap().as_str().into(),
                        ),
                        Rule::All => (None, inner.next().unwrap().as_str().into()),
                        _ => (None, String::new()),
                    };

                    to_push_at_last = Some(AST {
                        node: NodeKind::All {
                            all_or_names: elements.0,
                            content: elements.1,
                        },
                        meta: NodeMeta {
                            span: Some(span.clone()),
                            alias: alias.clone(),
                        },
                    });
                }
                Rule::Sentences => {
                    let mut inner = pair.into_inner();

                    let alias = take_alias(&mut inner);

                    let sentences: Vec<_> = inner
                        .filter(|p| p.as_rule() == Rule::Sen)
                        .map(|p| p.into_inner().next().unwrap().as_str().to_string())
                        .collect();

                    to_push_at_last = Some(AST {
                        meta: NodeMeta {
                            span: Some(span.clone()),
                            alias: alias.clone(),
                        },
                        node: NodeKind::Sen(sentences),
                    });
                }
                Rule::Selector => {
                    let mut inner = pair.into_inner();

                    let local = match inner.peek() {
                        Some(p) if p.as_rule() == Rule::Slash => {
                            inner.next();
                            true
                        }
                        _ => false,
                    };

                    let mut path = vec![];
                    let mut trailing_dot = false;
                    for p in inner {
                        match p.as_rule() {
                            Rule::Ident => {
                                path.push(p.as_str().to_string());
                            }
                            Rule::LastDot => {
                                trailing_dot = true;
                            }
                            _ => {}
                        }
                    }

                    to_push_at_last = Some(AST {
                        meta: NodeMeta {
                            span: Some(span),
                            alias: None,
                        },
                        node: NodeKind::Selector {
                            local,
                            path,
                            trailing_dot,
                        },
                    });
                }
                _ => (),
            }

            if let Some(to_add) = to_push_at_last {
                if let Some(last) = ast.last_mut() {
                    let (_, a, v) = last.take_mut_section_like().unwrap();

                    if let Some(ref alias) = to_add.meta.alias {
                        check_alias_conflict(
                            alias,
                            a,
                            v,
                            v.len(),
                            to_add.get_span().unwrap(),
                            &mut errs,
                        );
                    }

                    v.push(to_add);
                }
            }
        }

        while ast.len() > 1 {
            let to_add = ast.pop().unwrap();

            if let Some(last) = ast.last_mut() {
                let (_, a, v) = last.take_mut_section_like().unwrap();

                if let Some(ref alias) = to_add.meta.alias {
                    check_alias_conflict(
                        alias,
                        a,
                        v,
                        v.len(),
                        to_add.get_span().unwrap(),
                        &mut errs,
                    );
                }

                v.push(to_add);
            }
        }

        if let Some(names) = &names {
            fn check_conflict_with_names(names: &Vec<String>, ast: &AST) -> Vec<(Span, String)> {
                let (alias, children) = ast.take_section_like().unwrap();
                let mut v = vec![];
                for n in names {
                    if let Some(index) = alias.get(n) {
                        v.push((children[*index].get_span().unwrap(), n.clone()));
                    }
                }
                for p in children {
                    if let NodeKind::Section { .. } = &p.node {
                        v.extend(check_conflict_with_names(names, p));
                    }
                }
                v
            }
            for (span, name) in check_conflict_with_names(names, &ast[0]) {
                errs.push(ParseError::AliasConflictWithNames(name, span));
            }
        }

        // TODO: Selectorの妥当性

        let names = if let Some(names) = names {
            names
        } else {
            // エラーを追加してからのほうが優しい
            errs.push(ParseError::MissingNames);
            return Err(errs);
        };

        if !errs.is_empty() {
            return Err(errs);
        }

        Ok(Document {
            names,
            ast: ast.into_iter().next().unwrap(),
        })
    }
}

type Alias = FxHashMap<String, usize>;

#[derive(Debug)]
pub struct NodeMeta {
    span: Option<Span>,
    alias: Option<String>,
}

#[derive(Debug)]
pub enum NodeKind {
    ///  Contents
    Sen(Vec<String>),
    /// All or Name, Content
    All {
        all_or_names: Option<Vec<String>>,
        content: String,
    },
    ///  depth,  Content, Children
    Section {
        level: usize,
        content: String,
        aliases: Alias,
        children: Vec<AST>,
    },
    Top {
        aliases: Alias,
        children: Vec<AST>,
    },
    /// local, paths, last dot
    Selector {
        local: bool,
        path: Vec<String>,
        trailing_dot: bool,
    },
}

#[derive(Debug)]
pub struct AST {
    pub node: NodeKind,
    pub meta: NodeMeta,
}

fn take_alias(inner: &mut Pairs<'_, Rule>) -> Option<String> {
    let alias = inner
        .peek()
        .filter(|p| p.as_rule() == Rule::Ident)
        .map(|p| p.as_str().to_string());
    if alias.is_some() {
        inner.next();
    }
    alias
}

fn check_alias_conflict(
    alias: &str,
    aliases: &mut FxHashMap<String, usize>,
    children: &[AST],
    new_index: usize,
    new_span: Span,
    errs: &mut Vec<ParseError>,
) {
    if let Some(conflict_index) = aliases.insert(alias.to_string(), new_index) {
        errs.push(ParseError::DuplicateAlias(alias.to_string(), new_span));
        // TODO:
        // これだと複数回被ったときに最初と最後以外、2重のエラーが出る
        errs.push(ParseError::DuplicateAlias(
            alias.to_string(),
            children[conflict_index].get_span().unwrap(),
        ));
    }
}

impl AST {
    fn take_mut_section_like(&mut self) -> Option<(usize, &mut Alias, &mut Vec<AST>)> {
        match &mut self.node {
            NodeKind::Top {
                aliases: a,
                children: v,
            } => Some((0, a, v)),
            NodeKind::Section {
                aliases: a,
                children: v,
                content: _,
                level: d,
            } => Some((*d, a, v)),
            _ => None,
        }
    }

    fn take_section_like(&self) -> Option<(&Alias, &Vec<AST>)> {
        match &self.node {
            NodeKind::Top {
                aliases: a,
                children: v,
            } => Some((a, v)),
            NodeKind::Section {
                aliases: a,
                children: v,
                ..
            } => Some((a, v)),
            _ => None,
        }
    }

    fn get_span(&self) -> Option<Span> {
        self.meta.span.clone()
    }
}

#[cfg(test)]
mod tests {

    use crate::parser::{Document, ParseError, Rule, SandParser};
    use pest::Parser as _;

    /// Helper to parse input into Document or capture errors.
    fn parse_doc(input: &str) -> Result<Document, Vec<ParseError>> {
        let pairs = SandParser::parse(Rule::doc, input).unwrap();
        pairs.try_into()
    }

    #[test]
    fn simple_parse() {
        let doc = r#"
#(en, ja)

## Title
Content
"#;
        assert!(parse_doc(doc).is_ok(), "Expected simple doc to parse");
    }

    #[test]
    fn missing_names_error() {
        let doc = r#"
## Section without names
Content
"#;
        let err = parse_doc(doc).unwrap_err();
        assert!(
            matches!(err.as_slice(), [ParseError::MissingNames]),
            "Expected MissingNames error"
        );
    }

    #[test]
    fn duplicate_names_error() {
        let doc = r#"
#(en, en)
## Section
Content
"#;
        let errs = parse_doc(doc).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ParseError::DuplicateNames(name, _) if name == "en")),
            "Expected DuplicateNames error"
        );
    }

    #[test]
    fn duplicate_alias_error() {
        let doc = r#"
#(en)
#s1[One][一]
#s1[Two][二]
"#;
        let errs = parse_doc(doc).unwrap_err();
        assert!(
            errs.iter()
                .filter(|e| matches!(e, ParseError::DuplicateAlias(_, _)))
                .count()
                >= 1,
            "Expected at least one DuplicateAlias error"
        );
    }

    #[test]
    fn alias_conflict_with_names() {
        // alias 'en' conflicts with declared name 'en'
        let doc = r#"
#(en, ja)

#en[Test][テスト]
"#;
        let errs = parse_doc(doc).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| matches!(e, ParseError::AliasConflictWithNames(..))),
            "Expected AliasConflictWithNames for 'en'"
        );
    }

    #[test]
    fn parse_apply_all_and_sentences_and_selector() {
        let doc = r#"
#(en)
#hello# Section

A section.

#{all, { content }}

#sents[One][Two]

#.hello.sents.en
"#;
        let result = parse_doc(doc);
        assert!(
            result.is_ok(),
            "Expected apply-all, sentences, and selector to parse correctly"
        );
    }
}
