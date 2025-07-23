use pest::iterators::Pairs;
use pest_derive::Parser;
use rustc_hash::FxHashMap;

#[derive(Parser)]
#[grammar = "sand.pest"]
pub struct SandParser;

#[derive(Debug)]
pub struct Document {
    names: Vec<String>,
    ast: AST,
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
    DuplicateNames(Span),
    #[error("aliases are duplicated: {0}")]
    DuplicateAlias(String, Span),
    #[error("aliases and names are conflicted: {0}")]
    AliasConflictWithNames(String, Span),
    #[error("names are not defined")]
    MissingNames,
}

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
                        errs.push(ParseError::DuplicateNames(span)); // TODO: これだとdupのと一貫性がないかも
                    }
                    let ident_list_pair = pair.into_inner().next().unwrap();

                    names = Some(
                        ident_list_pair
                            .into_inner()
                            .filter(|p| p.as_rule() == Rule::Ident)
                            .map(|p| p.as_str().to_string())
                            .collect(),
                    );
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
                vec![]
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
    use crate::parser::{Document, Rule, SandParser};
    use pest::Parser as _;

    #[test]
    fn simple_parse() -> Result<(), Box<dyn std::error::Error>> {
        let parsed = SandParser::parse(
            // TODO: 個分け
            Rule::doc,
            r#"
#(en, ja)

つまり何個でも伸ばせる。ただしこの定義は一つ。先頭にあるといい

#sentence## 文

markdownへの変換のみに関連するaliasつきのセクション

#s1[
	Thank you!
][
	ありがとう！
]

### セレクター

エイリアスなし
\#\# セレクター

#s2[
	I am sleepy.
][
	眠いです。
]

#.sentence.s1. に関連するんだけどさ〜(ここでKかgdで情報表示)。#.sentence.s1.ja 違う

トップレベルのja, enみたいなのは省略可能(すべてを指す)

#.
これで全ての文書
#.en
英文全体
#.ja
ここでは日本語文全体

#./s2. みたいにして相対

#[
	I got it.
][
	よし！
]

#./0.ja のようにもできる。./は最初の.のあとにだけ可能性がある。
セクションごとに0-indexedにふる。補完を出したい。

#{{ \n }}

みたいにすることで全体に適用できる。[]の中は改行もtrimするから、改行をいれるには\nがいる。

#{{ \n }}

は

#{all, { \n }}

に意味的に等しく、

#{[ja], { \n }}

ともできる
            "#,
        );

        match parsed {
            Ok(p) => {
                let a: Result<Document, _> = dbg!(p.try_into());
                assert!(a.is_ok())
            }
            Err(ref e) => {
                eprintln!("{e}");
                parsed?;
            }
        }
        Ok(())
    }
}
