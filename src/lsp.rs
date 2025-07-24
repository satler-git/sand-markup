use crate::parser::{AST, Document, NodeKind, Rule};
use rustc_hash::FxHashMap;
use tokio::sync::Mutex;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::parser::{ParseError, Span};

#[derive(Debug)]
pub struct SandServer {
    pub client: Client,

    document_map: Mutex<FxHashMap<Url, String>>,
}

fn byte_offset_to_position(text: &str, offset: usize) -> Position {
    let mut line = 0;
    let mut utf16_char_offset = 0;
    for (i, c) in text.char_indices() {
        if i == offset {
            break;
        }
        if c == '\n' {
            line += 1;
            utf16_char_offset = 0;
        } else {
            utf16_char_offset += c.len_utf16();
        }
    }
    Position {
        line: line as u32,
        character: utf16_char_offset as u32,
    }
}

fn position_to_byte_offset(text: &str, position: &Position) -> usize {
    let mut current_line = 0;
    let mut utf16_char_offset = 0;
    let mut byte_offset = 0;

    for (i, c) in text.char_indices() {
        if current_line == position.line && utf16_char_offset == position.character {
            return i;
        }

        if c == '\n' {
            current_line += 1;
            utf16_char_offset = 0;
        } else {
            utf16_char_offset += c.len_utf16() as u32;
        }
        byte_offset = i + c.len_utf8();
    }

    if current_line == position.line && utf16_char_offset == position.character {
        return byte_offset;
    }

    text.len()
}

fn pos_to_ast<'a>(text: &str, pos: &'a Position, ast: &'a AST) -> Option<&'a AST> {
    let offset = position_to_byte_offset(text, pos);

    ast.find_node_at_position(offset)
}

fn convert_pest_error_to_diagnostic(
    file_content: &str,
    error: pest::error::Error<Rule>,
) -> Diagnostic {
    let span = {
        let (start, end) = match error.location {
            pest::error::InputLocation::Pos(pos) => (pos, pos + 1),
            pest::error::InputLocation::Span((s, e)) => (s, e),
        };
        Span { start, end }
    };

    let start_pos = byte_offset_to_position(file_content, span.start);
    let end_pos = byte_offset_to_position(file_content, span.end);

    Diagnostic {
        range: Range::new(start_pos, end_pos),
        severity: Some(DiagnosticSeverity::ERROR),
        code: None,
        source: Some("Sand Parser".to_string()),
        message: error.variant.message().to_string(),
        related_information: None,
        tags: None,
        data: None,
        code_description: None,
    }
}

fn convert_parse_error_to_diagnostic(file_content: &str, error: ParseError) -> Diagnostic {
    let (span, message) = match &error {
        ParseError::MultipleNameDefine(span)
        | ParseError::DuplicateNames(_, span)
        | ParseError::DuplicateAlias(_, span)
        | ParseError::AliasConflictWithNames(_, span)
        | ParseError::Selector(_, span) => (span.clone(), error.to_string()),
        ParseError::MissingNames => (Span { start: 0, end: 1 }, error.to_string()),
    };

    let start_pos = byte_offset_to_position(file_content, span.start);
    let end_pos = byte_offset_to_position(file_content, span.end);

    Diagnostic {
        range: Range::new(start_pos, end_pos),
        severity: Some(DiagnosticSeverity::ERROR),
        code: None,
        source: Some("Sand Validator".to_string()),
        message,
        related_information: None,
        tags: None,
        data: None,
        code_description: None,
    }
}

fn convert_parse_errors_to_diagnostics(
    file_content: &str,
    errors: Vec<ParseError>,
) -> Vec<Diagnostic> {
    errors
        .into_iter()
        .map(|err| convert_parse_error_to_diagnostic(file_content, err))
        .collect()
}

impl SandServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            document_map: Mutex::new(FxHashMap::default()),
        }
    }

    fn generate_diagnostics(text: &str) -> Vec<Diagnostic> {
        use crate::parser::{Document, Rule, SandParser};
        use pest::Parser as _;

        let pairs = SandParser::parse(Rule::doc, text);

        let mut diagnostics = vec![];

        match pairs {
            Err(parsing_error) => {
                diagnostics.push(convert_pest_error_to_diagnostic(text, parsing_error));
            }
            Ok(pairs) => {
                let doc: std::result::Result<Document, _> = pairs.try_into();

                if let Err(errs) = doc {
                    diagnostics.extend(convert_parse_errors_to_diagnostics(text, errs));
                }
            }
        }

        diagnostics
    }

    async fn publish_diagnostics(&self, uri: Url, text: String) {
        self.client
            .publish_diagnostics(uri, Self::generate_diagnostics(&text), None)
            .await;
    }

    async fn parse(&self, url: &Url) -> Result<Document> {
        use crate::parser::{Rule, SandParser};
        use pest::Parser as _;
        use tower_lsp::jsonrpc::{Error, ErrorCode};

        let map = self.document_map.lock().await;

        let text: &String = map.get(url).ok_or(Error {
            code: ErrorCode::InvalidParams,
            message: "failed to find text document in our map".into(),
            data: None,
        })?;

        let pairs = SandParser::parse(Rule::doc, text).map_err(|err| Error {
            code: ErrorCode::ParseError,
            message: err.variant.message().to_string().into(),
            data: None,
        })?;

        pairs.try_into().map_err(|errs: Vec<ParseError>| Error {
            code: ErrorCode::ParseError,
            message: format!(
                "Parse validation failed: {}",
                errs.iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join("; ")
            )
            .into(),
            data: None,
        })
    }
}

mod _doc {
    pub(super) const SECTION_DOC: &str = r#"
The `Section` syntax provides a way to structure documents by creating meaningful divisions within your text. Currently, its primary purpose is to define logical sections, which can optionally include an alias.

Here's a quick breakdown with examples:

```sand
#sec1# This is an aliased Level 1 Section
## This is a Level 1 Section without an alias
#sec2## This is an aliased Level 2 Section, nested under a Level 1 Section
### This is a Level 2 Section without an alias
```

In the examples above:

  * The hashes (##) determine the level of the section. Two hashes (##) indicate a Level 1 Section, three hashes (###) indicate a Level 2 Section, and so on.
  * The optional **`Ident`** (like `sec1` or `sec2`) acts as an **alias** for the section. This alias can be used for quick referencing or navigation within your document.
  * The content of the section must be on a single line with a line break at the end.
"#;

    pub(super) const ALL_DOC: &str = r#"
`ApplyAll` syntax, Apply a piece of content under all or a selected list of contexts (e.g. locales, formats).

* **Sugar form:**

```sand
#{{ Use this everywhere }}

#all{{ You can use with alias. }}
```

is equivalent to

```sand
#{all, { Use this everywhere }}
#all{all, { You can use with alias. }}
```
* **Targeted form:**

```sand
#{[en],{ Hello only in English }}
#{[mobile],{ Shown only on mobile }}
```

Here, the list inside `[...]` can be any identifiers you’ve defined (languages, output formats, etc.).
"#;

    pub(super) const SENTENCE_DOC: &str = r#"
**Parallel Sentences**
Use when you have one piece of content per declared name (e.g. multiple languages):

```sand
#(en, ja)    // Declare two targets: English and Japanese

#alias[
  Hello!
][
  こんにちは！
]
```

* You must provide exactly one sentence block **per** declared name, in the same order.
* The `Ident` (`alias`) is optional but useful for reference.
"#;

    pub(super) const SELECTOR_DOC: &str = r##"
**Selector**
Chooses one or more named contexts (e.g. languages, formats) relative to your current position.

* **Global vs. Local**

* `#.` or `#..` – selects all names from the document root.
* `#./foo.en` – starts from the *current* section (due to `/`) and picks `foo` → `en`.
* Without `/`, selection begins at the document root.

* **Identifiers & Indexes**

* You can use either an **alias** or a zero‑based **index** to refer to each level.
* Example: these are equivalent:

```sand
#(en, ja)

#sec1# level 1
#sec2## level 2

#test[
    Hello!
][
    こんにちは
]

#./test.en            // local from current section
#.0.0.0.en            // index-based from root (sec1=0, sec2=0, test=0)
#./0.en               // index-based local
```

* **Trailing Dot (`.`)**

* A selector ending in `.` (e.g. `#.sec1.sec2.`) expands to *all* declared names, as if you had written one selector per name:

```sand
#.sec1.sec2.   // same as #.sec1.sec2.en and #.sec1.sec2.ja
```

* **Minimal Forms**

* `#.` or `#..` with nothing else simply means “select every name” in the appropriate scope (global or local).
"##;
}

#[tower_lsp::async_trait]
impl LanguageServer for SandServer {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "SandServer".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::FULL),
                        will_save: Some(false),
                        will_save_wait_until: Some(false),
                        save: Some(TextDocumentSyncSaveOptions::Supported(true)),
                    },
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                ..Default::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "server initialized!")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let mut map = self.document_map.lock().await;
        map.insert(
            params.text_document.uri.clone(),
            params.text_document.text.clone(),
        );
        self.client
            .log_message(
                MessageType::INFO,
                format!("file opened: {}", params.text_document.uri),
            )
            .await;
        self.publish_diagnostics(params.text_document.uri, params.text_document.text)
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;

        let new_text = params
            .content_changes
            .into_iter()
            .next()
            .map(|change| change.text);

        if let Some(text) = new_text {
            let mut map = self.document_map.lock().await;
            map.insert(uri.clone(), text.clone());
            self.client
                .log_message(
                    MessageType::INFO,
                    format!("file changed: {uri} (version: {version})"),
                )
                .await;

            self.publish_diagnostics(uri, text).await;
        } else {
            self.client
                .log_message(
                    MessageType::WARNING,
                    "didChange received without full text content (incremental sync not fully supported)",
                )
                .await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let mut map = self.document_map.lock().await;
        map.remove(&params.text_document.uri);
        self.client
            .log_message(
                MessageType::INFO,
                format!("file closed: {}", params.text_document.uri),
            )
            .await;
        self.client
            .publish_diagnostics(params.text_document.uri, Vec::new(), None)
            .await;
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        use tower_lsp::jsonrpc::{Error, ErrorCode};

        let doc = self
            .parse(&params.text_document_position_params.text_document.uri)
            .await?;

        let map = self.document_map.lock().await;
        let text: &String = map
            .get(&params.text_document_position_params.text_document.uri)
            .ok_or(Error {
                code: ErrorCode::InvalidParams,
                message: "failed to find text document in our map".into(),
                data: None,
            })?;

        Ok(pos_to_ast(
            text,
            &params.text_document_position_params.position,
            &doc.ast,
        )
        .and_then(|ast| match &ast.node {
            NodeKind::Sen(_) => Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: _doc::SENTENCE_DOC.into(),
                }),
                range: None,
            }),
            NodeKind::All { .. } => Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: _doc::ALL_DOC.into(),
                }),
                range: None,
            }),
            NodeKind::Section { .. } => Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: _doc::SECTION_DOC.into(),
                }),
                range: None,
            }),
            NodeKind::Selector { local, .. } => {
                // どうにかして親を取得
                let target_ast = if *local {
                    let parent = doc.ast.find_parent_at_position(position_to_byte_offset(
                        text,
                        &params.text_document_position_params.position,
                    ));
                    if let Some(parent) = parent {
                        parent.clone()
                    } else {
                        eprintln!("failed to find the parent"); // TODO: error log
                        return None;
                    }
                } else {
                    doc.ast.clone()
                };

                let rendered = crate::formatter::render_plain(
                    &Document {
                        names: doc.names,
                        ast: target_ast,
                    },
                    &crate::formatter::Selector(ast.clone()),
                )
                .join("\n\n---\n\n");

                Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,

                        value: format!("{rendered}\n\n---\n\n{}", _doc::SELECTOR_DOC),
                    }),

                    range: None,
                })
            }
            _ => None,
        }))
    }
}
