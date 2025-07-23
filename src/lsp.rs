use crate::parser::Rule;
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
    let mut character = 0;
    for (i, char_code) in text.char_indices() {
        if i == offset {
            break;
        }
        if char_code == '\n' {
            line += 1;
            character = 0;
        } else {
            character += 1;
        }
    }
    Position {
        line: line as u32,
        character: character as u32,
    }
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
        | ParseError::AliasConflictWithNames(_, span) => (span.clone(), error.to_string()),
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

        let pairs = SandParser::parse(Rule::doc, &text);

        let mut diagnostics = vec![];

        match pairs {
            Err(parsing_error) => {
                diagnostics.push(convert_pest_error_to_diagnostic(&text, parsing_error));
            }
            Ok(pairs) => {
                let doc: std::result::Result<Document, _> = pairs.try_into();

                match doc {
                    Err(errs) => {
                        diagnostics.extend(convert_parse_errors_to_diagnostics(&text, errs));
                    }
                    _ => {}
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
}

#[tower_lsp::async_trait]
impl LanguageServer for SandServer {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "SandServer".to_string(),
                version: Some("0.1.0".to_string()),
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
                    format!("file changed: {} (version: {})", uri, version),
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
}
