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

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
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
#[derive(Error, Debug, Hash, PartialEq, Eq)]
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
    #[error("selector is incorrect: {0}")]
    Selector(SelectorError, Span),
}

#[derive(Error, Debug, Hash, PartialEq, Eq)]
pub enum SelectorError {
    #[error("the last keyword is not dot or names")]
    LastIsNotDotOrName,
    #[error("the number points outside the index.")]
    OutOfIndex,
    #[error("neither a number nor an alias: {0}")]
    Neither(String),
    #[error("expected to be global selector , but found a local selector")]
    Local,
}

pub fn validate_non_local_selector(doc: &Document, sel: &AST) -> Vec<ParseError> {
    let mut v = vec![];
    if let NodeKind::Selector {
        local,
        path,
        trailing_dot,
    } = &sel.node
    {
        if *local {
            v.push(ParseError::Selector(
                SelectorError::Local,
                sel.get_span().unwrap(),
            ));
            return v;
        }

        let range = if !trailing_dot && !path.is_empty() {
            if !doc.names.contains(path.last().unwrap()) {
                v.push(ParseError::Selector(
                    SelectorError::LastIsNotDotOrName,
                    sel.get_span().unwrap(),
                ));
            }
            0..(path.len() - 1)
        } else {
            0..(path.len())
        };

        let mut curr = &doc.ast;

        for k in &path[range] {
            if matches!(curr.node, NodeKind::Sen { .. })
                || matches!(curr.node, NodeKind::All { .. })
            {
                break;
            }
            let (alias, children) = curr.take_section_like().unwrap();
            let children_without_sel: Vec<&AST> = children
                .iter()
                .filter(|p| !matches!(&p.node, NodeKind::Selector { .. }))
                .collect();

            if let Some(index) = alias.get(k) {
                curr = children_without_sel[*index];
            } else if let Ok(index) = k.parse::<usize>() {
                if index >= children_without_sel.len() {
                    v.push(ParseError::Selector(
                        SelectorError::OutOfIndex,
                        sel.get_span().unwrap(),
                    ));
                    break;
                } else {
                    curr = children_without_sel[index];
                }
            } else {
                v.push(ParseError::Selector(
                    SelectorError::Neither(k.clone()),
                    sel.get_span().unwrap(),
                ));
                break;
            }
        }
    }
    v
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
        let mut names: Option<(Span, Vec<String>)> = None;

        let mut errs = FxHashSet::default();

        let root = pairs.next().unwrap();

        for pair in root.into_inner() {
            let span: Span = pair.as_span().into();

            let mut to_push_at_last = None;

            match pair.as_rule() {
                Rule::PartName => {
                    if let Some((prev_span, _)) = names {
                        errs.insert(ParseError::MultipleNameDefine(prev_span.clone()));
                        errs.insert(ParseError::MultipleNameDefine(span.clone()));
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
                            errs.insert(ParseError::DuplicateNames(name.clone(), span.clone()));
                        }
                    }

                    names = Some((span, raw_names));
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
                    to_push_at_last = Some(parse_selector(span, pair));
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

        if let Some((_, names)) = &names {
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
                errs.insert(ParseError::AliasConflictWithNames(name, span));
            }
        }

        // Selectorの妥当性
        if let Some((_, names)) = &names {
            fn check_selector(names: &Vec<String>, top_ast: &AST, ast: &AST) -> Vec<ParseError> {
                let (_, children) = ast.take_section_like().unwrap();
                let mut v = vec![];
                for p in children {
                    if let NodeKind::Selector {
                        local,
                        path,
                        trailing_dot,
                    } = &p.node
                    {
                        let range = if !trailing_dot && !path.is_empty() {
                            if !names.contains(path.last().unwrap()) {
                                v.push(ParseError::Selector(
                                    SelectorError::LastIsNotDotOrName,
                                    p.get_span().unwrap(),
                                ));
                            }
                            0..(path.len() - 1)
                        } else {
                            0..(path.len())
                        };

                        let mut curr = if *local { ast } else { top_ast };

                        for k in &path[range] {
                            if matches!(curr.node, NodeKind::Sen { .. })
                                || matches!(curr.node, NodeKind::All { .. })
                            {
                                break;
                            }
                            let (alias, children) = curr.take_section_like().unwrap();
                            let children_without_sel: Vec<&AST> = children
                                .iter()
                                .filter(|p| !matches!(&p.node, NodeKind::Selector { .. }))
                                .collect();

                            if let Some(index) = alias.get(k) {
                                curr = children_without_sel[*index];
                            } else if let Ok(index) = k.parse::<usize>() {
                                if index >= children_without_sel.len() {
                                    v.push(ParseError::Selector(
                                        SelectorError::OutOfIndex,
                                        p.get_span().unwrap(),
                                    ));
                                    break;
                                } else {
                                    curr = children_without_sel[index];
                                }
                            } else {
                                v.push(ParseError::Selector(
                                    SelectorError::Neither(k.clone()),
                                    p.get_span().unwrap(),
                                ));
                                break;
                            }
                        }
                    }

                    if let NodeKind::Section { .. } = &p.node {
                        v.extend(check_selector(names, top_ast, p));
                    }
                }
                v
            }
            errs.extend(check_selector(names, &ast[0], &ast[0]));
        }

        let names = if let Some(names) = names {
            names.1
        } else {
            // エラーを追加してからのほうが優しい
            errs.insert(ParseError::MissingNames);
            return Err(errs.into_iter().collect());
        };

        if !errs.is_empty() {
            return Err(errs.into_iter().collect());
        }

        Ok(Document {
            names,
            ast: ast.into_iter().next().unwrap(),
        })
    }
}

pub fn parse_selector(span: Span, pair: pest::iterators::Pair<'_, Rule>) -> AST {
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
    AST {
        meta: NodeMeta {
            span: Some(span),
            alias: None,
        },
        node: NodeKind::Selector {
            local,
            path,
            trailing_dot,
        },
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
    errs: &mut FxHashSet<ParseError>,
) {
    if let Some(conflict_index) = aliases.insert(alias.to_string(), new_index) {
        errs.insert(ParseError::DuplicateAlias(alias.to_string(), new_span));
        errs.insert(ParseError::DuplicateAlias(
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

    pub(crate) fn take_section_like(&self) -> Option<(&Alias, &Vec<AST>)> {
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
