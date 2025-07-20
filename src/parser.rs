use anyhow::{Error, anyhow};
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

// TODO: エラーを作る
impl TryFrom<Pairs<'_, Rule>> for Document {
    type Error = Vec<(Span, Error)>;

    fn try_from(mut pairs: Pairs<'_, Rule>) -> Result<Self, Vec<(Span, Error)>> {
        let mut ast = vec![AST {
            node: NodeKind::Top {
                ailiases: FxHashMap::default(),
                children: vec![],
            },
            meta: NodeMeta {
                ailias: None,
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
                        errs.push((span, anyhow!("names are defined more than once")));
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

                    let ailias = take_ailias(&mut inner);

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

                            if let Some(ref ailias) = top.meta.ailias {
                                if let Some(conflict_index) = a.insert(ailias.clone(), v.len()) {
                                    errs.push((
                                        top.get_span().unwrap(),
                                        anyhow!("aliases are duplicated: {ailias}"),
                                    ));
                                    errs.push((
                                        v[conflict_index].get_span().unwrap(),
                                        anyhow!("aliases are duplicated: {ailias}"),
                                    ));
                                }
                            }

                            v.push(top);
                        }
                    }

                    ast.push(AST {
                        meta: NodeMeta {
                            span: Some(span),
                            ailias,
                        },
                        node: NodeKind::Section {
                            level,
                            content,
                            ailiases: FxHashMap::default(),
                            children: vec![],
                        },
                    });
                }
                Rule::ApplyAll => {
                    let mut inner = pair.into_inner();

                    let ailias = take_ailias(&mut inner);
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
                            ailias: ailias.clone(),
                        },
                    });
                }
                Rule::Sentences => {
                    let mut inner = pair.into_inner();

                    let ailias = take_ailias(&mut inner);

                    let sentences: Vec<_> = inner
                        .filter(|p| p.as_rule() == Rule::Sen)
                        .map(|p| p.into_inner().next().unwrap().as_str().to_string())
                        .collect();

                    to_push_at_last = Some(AST {
                        meta: NodeMeta {
                            span: Some(span.clone()),
                            ailias: ailias.clone(),
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
                            ailias: None,
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

                    if let Some(ref ailias) = to_add.meta.ailias {
                        if let Some(conflict_index) = a.insert(ailias.clone(), v.len()) {
                            errs.push((
                                to_add.get_span().unwrap(),
                                anyhow!("aliases are duplicated: {ailias}"),
                            ));
                            errs.push((
                                v[conflict_index].get_span().unwrap(),
                                anyhow!("aliases are duplicated: {ailias}"),
                            ));
                        }
                    }

                    v.push(to_add);
                }
            }
        }

        while ast.len() > 1 {
            let to_add = ast.pop().unwrap();

            if let Some(last) = ast.last_mut() {
                let (_, a, v) = last.take_mut_section_like().unwrap();

                if let Some(ref ailias) = to_add.meta.ailias {
                    if let Some(conflict_index) = a.insert(ailias.clone(), v.len()) {
                        errs.push((
                            to_add.get_span().unwrap(),
                            anyhow!("aliases are duplicated: {ailias}"),
                        ));
                        errs.push((
                            v[conflict_index].get_span().unwrap(),
                            anyhow!("aliases are duplicated: {ailias}"),
                        ));
                    }
                }

                v.push(to_add);
            }
        }

        if let Some(names) = &names {
            // TODO: エイリアスを走査してnamesと被ってないかチェック
        }

        // TODO: Selectorの妥当性

        let names = if let Some(names) = names {
            names
        } else {
            // エラーを追加してからのほうが優しい
            errs.push((Span { start: 0, end: 0 }, anyhow!("names are not defined")));
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

type Ailias = FxHashMap<String, usize>;

#[derive(Debug)]
pub struct NodeMeta {
    span: Option<Span>,
    ailias: Option<String>,
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
        ailiases: Ailias,
        children: Vec<AST>,
    },
    Top {
        ailiases: Ailias,
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

fn take_ailias(inner: &mut Pairs<'_, Rule>) -> Option<String> {
    let ailias = inner
        .peek()
        .filter(|p| p.as_rule() == Rule::Ident)
        .map(|p| p.as_str().to_string());
    if ailias.is_some() {
        inner.next();
    }
    ailias
}

impl AST {
    fn take_mut_section_like(&mut self) -> Option<(usize, &mut Ailias, &mut Vec<AST>)> {
        match &mut self.node {
            NodeKind::Top {
                ailiases: a,
                children: v,
            } => Some((0, a, v)),
            NodeKind::Section {
                ailiases: a,
                children: v,
                content: _,
                level: d,
            } => Some((*d, a, v)),
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
