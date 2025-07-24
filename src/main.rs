use anyhow::Result;

use sand::parser::{Document, ParseError, Rule, Span};

use std::path::PathBuf;
use tokio::{fs::File, io::AsyncReadExt};

use clap::{CommandFactory, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Parse and validate the given input file.
    ///
    /// Reads the specified file, parses its contents according to the
    /// grammar rules, and reports any errors or warnings found.
    Parse {
        /// Path to the input file to be parsed.
        ///
        /// Must point to a readable file containing the source to validate.
        #[arg(value_name = "FILE", value_parser)]
        input: PathBuf,
    },

    /// Launch the Language Server Protocol (LSP) server.
    ///
    /// Starts the LSP server, allowing IDEs and editors to connect
    /// for on‑the‑fly diagnostics, completions(to do), and other language features.
    Lsp,

    /// Generate shell completion scripts.
    ///
    /// Outputs a shell-specific completion script to stdout.
    /// Supported shells include Bash, Zsh, Fish, PowerShell, and Elvish.
    Completions { shell: clap_complete::Shell },

    /// Render filtered document output based on a selector.
    ///
    /// Extracts and displays specific content from the document based on
    /// the provided selector syntax.
    Out {
        /// Selector string to filter document content.
        ///
        /// Uses dot-notation to navigate the document structure.
        selector: String,
        /// Path to the input file to process.
        #[arg(long, short, value_name = "FILE", value_parser)]
        input: PathBuf,

        /// Output as Markdown Text
        #[arg(long, short)]
        markdown: bool,
    },
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
        ParseError::Selector(sel, span) => Diagnostic::error()
            .with_message(format!("selector syntax is incorrect: {sel}"))
            .with_labels(vec![
                Label::primary(file_id, span.start..span.end)
                    .with_message("selector syntax is incorrect"),
            ]),
        ParseError::MissingNames => Diagnostic::error().with_message("names are not defined"),
    }
}

pub fn convert_pest_error(
    file_id: usize,
    error: pest::error::Error<sand::parser::Rule>,
) -> Diagnostic<usize> {
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

use codespan_reporting::files::SimpleFiles;

fn report(files: &SimpleFiles<String, String>, diag: Diagnostic<usize>) {
    use codespan_reporting::term::{Config, emit, termcolor};

    let writer = termcolor::StandardStream::stderr(termcolor::ColorChoice::Auto);
    let config = Config::default();
    emit(&mut writer.lock(), &config, files, &diag)
        .unwrap_or_else(|e| eprintln!("failed to emit diagnostics: {e}"));
}

fn parse_with_reporting<'a, T, F>(rule: Rule, input: &'a str, filename: &str, f: F) -> T
where
    F: FnOnce(
        &mut SimpleFiles<String, String>,
        usize,
        pest::iterators::Pairs<'a, sand::parser::Rule>,
    ) -> Result<T, Vec<ParseError>>,
{
    use pest::Parser as _;

    let pairs = sand::parser::SandParser::parse(rule, input);
    let mut files = SimpleFiles::new();
    let file_id = files.add(filename.to_string(), input.to_string());

    let pairs = match pairs {
        Err(e) => {
            let diag = convert_pest_error(file_id, e);
            report(&files, diag);
            std::process::exit(1)
        }
        Ok(p) => p,
    };

    match f(&mut files, file_id, pairs) {
        Ok(val) => val,
        Err(errs) => {
            for err in errs {
                let diag = convert_parse_error(file_id, &err);
                report(&files, diag);
            }
            std::process::exit(1)
        }
    }
}

fn convert_to_doc_displaying_errs(input: &str, filename: &str) -> Document {
    parse_with_reporting(Rule::doc, input, filename, |_, _, pairs| pairs.try_into())
}

fn convert_to_sel_displaying_errs(
    input: &str,
    doc: &Document,
    filename: &str,
) -> sand::formatter::Selector {
    parse_with_reporting(Rule::Selector, input, filename, |_, _, pairs| {
        (doc, pairs).try_into()
    })
}

pub fn print_completions<G: clap_complete::Generator>(g: G) {
    let mut cmd = Args::command();
    let name = cmd.get_name().to_string();
    clap_complete::generate(g, &mut cmd, name, &mut std::io::stdout());
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Command::Parse { input } => {
            let mut file = File::open(&input).await?;

            let mut contents = String::new();
            file.read_to_string(&mut contents).await?;

            let filename = input.display().to_string();
            let doc = convert_to_doc_displaying_errs(&contents, &filename);
            println!("{doc:?}");
        }
        Command::Lsp => {
            use sand::lsp::SandServer;
            use tower_lsp::{LspService, Server};

            let stdin = tokio::io::stdin();
            let stdout = tokio::io::stdout();

            let (service, socket) = LspService::new(SandServer::new);
            Server::new(stdin, stdout, socket).serve(service).await;
        }
        Command::Completions { shell } => {
            print_completions(shell);
        }
        Command::Out {
            selector,
            markdown,
            input,
        } => {
            let mut file = File::open(&input).await?;

            let mut contents = String::new();
            file.read_to_string(&mut contents).await?;

            let filename = input.display().to_string();
            let doc = convert_to_doc_displaying_errs(&contents, &filename);
            let sel = convert_to_sel_displaying_errs(&selector, &doc, "<user>");

            let rendered = sand::formatter::render_plain(&doc, &sel, markdown);
            if rendered.len() == 1 {
                println!("{}", rendered[0]);
            } else {
                let width = terminal_size::terminal_size()
                    .map(|(w, _h)| match w {
                        terminal_size::Width(w) => w as usize,
                    })
                    .unwrap_or(80);

                for (content, name) in rendered.into_iter().zip(doc.names.iter()) {
                    use colored::Colorize;

                    let bar = "─".repeat(width.saturating_sub(name.len() + 1));

                    println!("{} {bar}", name.bold().underline().red());
                    println!();
                    println!("{content}");
                    println!();
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sand::parser::{ParseError, Span};
    use codespan_reporting::diagnostic::Diagnostic;

    // Note: Using standard Rust testing framework
    // Unit tests for pure functions in main.rs

    #[test]
    fn test_convert_parse_error_multiple_name_define_basic() {
        let span = Span { start: 5, end: 10 };
        let error = ParseError::MultipleNameDefine(span);
        let diagnostic = convert_parse_error(0, &error);
        
        assert_eq!(diagnostic.message, "names are defined more than once");
        assert_eq!(diagnostic.labels.len(), 1);
        assert_eq!(diagnostic.labels[0].range, 5..10);
        assert_eq!(diagnostic.labels[0].file_id, 0);
        assert_eq!(diagnostic.labels[0].message, "this is a repeated definition");
    }

    #[test]
    fn test_convert_parse_error_duplicate_names_basic() {
        let span = Span { start: 0, end: 5 };
        let error = ParseError::DuplicateNames("test".to_string(), span);
        let diagnostic = convert_parse_error(1, &error);
        
        assert_eq!(diagnostic.message, "duplicate name: `test`");
        assert_eq!(diagnostic.labels.len(), 1);
        assert_eq!(diagnostic.labels[0].range, 0..5);
        assert_eq!(diagnostic.labels[0].file_id, 1);
        assert_eq!(diagnostic.labels[0].message, "duplicate name here");
    }

    #[test]
    fn test_convert_parse_error_duplicate_alias_basic() {
        let span = Span { start: 10, end: 20 };
        let error = ParseError::DuplicateAlias("alias".to_string(), span);
        let diagnostic = convert_parse_error(2, &error);
        
        assert_eq!(diagnostic.message, "duplicate alias: `alias`");
        assert_eq!(diagnostic.labels.len(), 1);
        assert_eq!(diagnostic.labels[0].range, 10..20);
        assert_eq!(diagnostic.labels[0].file_id, 2);
        assert_eq!(diagnostic.labels[0].message, "duplicate alias here");
    }

    #[test]
    fn test_convert_parse_error_alias_conflict_basic() {
        let span = Span { start: 15, end: 25 };
        let error = ParseError::AliasConflictWithNames("conflict".to_string(), span);
        let diagnostic = convert_parse_error(3, &error);
        
        assert_eq!(diagnostic.message, "alias `conflict` conflicts with a name");
        assert_eq!(diagnostic.labels.len(), 1);
        assert_eq!(diagnostic.labels[0].range, 15..25);
        assert_eq!(diagnostic.labels[0].file_id, 3);
        assert_eq!(diagnostic.labels[0].message, "this alias conflicts with a name");
    }

    #[test]
    fn test_convert_parse_error_selector_basic() {
        let span = Span { start: 30, end: 40 };
        let error = ParseError::Selector("bad.selector".to_string(), span);
        let diagnostic = convert_parse_error(4, &error);
        
        assert_eq!(diagnostic.message, "selector syntax is incorrect: bad.selector");
        assert_eq!(diagnostic.labels.len(), 1);
        assert_eq!(diagnostic.labels[0].range, 30..40);
        assert_eq!(diagnostic.labels[0].file_id, 4);
        assert_eq!(diagnostic.labels[0].message, "selector syntax is incorrect");
    }

    #[test]
    fn test_convert_parse_error_missing_names_basic() {
        let error = ParseError::MissingNames;
        let diagnostic = convert_parse_error(5, &error);
        
        assert_eq!(diagnostic.message, "names are not defined");
        assert_eq!(diagnostic.labels.len(), 0);
    }

    #[test]
    fn test_convert_parse_error_with_empty_strings() {
        let span = Span { start: 0, end: 1 };
        
        let error1 = ParseError::DuplicateNames("".to_string(), span);
        let diagnostic1 = convert_parse_error(0, &error1);
        assert_eq!(diagnostic1.message, "duplicate name: ``");
        
        let error2 = ParseError::DuplicateAlias("".to_string(), span);
        let diagnostic2 = convert_parse_error(0, &error2);
        assert_eq!(diagnostic2.message, "duplicate alias: ``");
        
        let error3 = ParseError::Selector("".to_string(), span);
        let diagnostic3 = convert_parse_error(0, &error3);
        assert_eq!(diagnostic3.message, "selector syntax is incorrect: ");
        
        let error4 = ParseError::AliasConflictWithNames("".to_string(), span);
        let diagnostic4 = convert_parse_error(0, &error4);
        assert_eq!(diagnostic4.message, "alias `` conflicts with a name");
    }

    #[test]
    fn test_convert_parse_error_with_unicode_strings() {
        let span = Span { start: 0, end: 10 };
        
        let error1 = ParseError::DuplicateNames("тест".to_string(), span);
        let diagnostic1 = convert_parse_error(0, &error1);
        assert_eq!(diagnostic1.message, "duplicate name: `тест`");
        
        let error2 = ParseError::DuplicateAlias("別名".to_string(), span);
        let diagnostic2 = convert_parse_error(0, &error2);
        assert_eq!(diagnostic2.message, "duplicate alias: `別名`");
        
        let error3 = ParseError::Selector("선택자.경로".to_string(), span);
        let diagnostic3 = convert_parse_error(0, &error3);
        assert_eq!(diagnostic3.message, "selector syntax is incorrect: 선택자.경로");
    }

    #[test]
    fn test_convert_parse_error_with_special_characters() {
        let span = Span { start: 0, end: 5 };
        
        let error1 = ParseError::DuplicateNames("test-name_123".to_string(), span);
        let diagnostic1 = convert_parse_error(0, &error1);
        assert_eq!(diagnostic1.message, "duplicate name: `test-name_123`");
        
        let error2 = ParseError::Selector("path.to[0].item".to_string(), span);
        let diagnostic2 = convert_parse_error(0, &error2);
        assert_eq!(diagnostic2.message, "selector syntax is incorrect: path.to[0].item");
    }

    #[test]
    fn test_convert_parse_error_span_edge_cases() {
        // Test zero-width span
        let span1 = Span { start: 10, end: 10 };
        let error1 = ParseError::MultipleNameDefine(span1);
        let diagnostic1 = convert_parse_error(0, &error1);
        assert_eq!(diagnostic1.labels[0].range, 10..10);
        
        // Test large span
        let span2 = Span { start: 1000, end: 2000 };
        let error2 = ParseError::MultipleNameDefine(span2);
        let diagnostic2 = convert_parse_error(0, &error2);
        assert_eq!(diagnostic2.labels[0].range, 1000..2000);
        
        // Test single character span
        let span3 = Span { start: 5, end: 6 };
        let error3 = ParseError::MultipleNameDefine(span3);
        let diagnostic3 = convert_parse_error(0, &error3);
        assert_eq!(diagnostic3.labels[0].range, 5..6);
    }

    #[test]
    fn test_convert_parse_error_different_file_ids() {
        let span = Span { start: 0, end: 5 };
        let error = ParseError::MultipleNameDefine(span);
        
        let diagnostic1 = convert_parse_error(0, &error);
        let diagnostic2 = convert_parse_error(100, &error);
        let diagnostic3 = convert_parse_error(usize::MAX, &error);
        
        assert_eq!(diagnostic1.labels[0].file_id, 0);
        assert_eq!(diagnostic2.labels[0].file_id, 100);
        assert_eq!(diagnostic3.labels[0].file_id, usize::MAX);
        
        // All should have the same message and range
        assert_eq!(diagnostic1.message, diagnostic2.message);
        assert_eq!(diagnostic1.message, diagnostic3.message);
        assert_eq!(diagnostic1.labels[0].range, diagnostic2.labels[0].range);
        assert_eq!(diagnostic1.labels[0].range, diagnostic3.labels[0].range);
    }

    #[test]
    fn test_convert_parse_error_long_strings() {
        let span = Span { start: 0, end: 10 };
        let long_name = "a".repeat(1000);
        
        let error = ParseError::DuplicateNames(long_name.clone(), span);
        let diagnostic = convert_parse_error(0, &error);
        
        assert_eq!(diagnostic.message, format!("duplicate name: `{}`", long_name));
        assert_eq!(diagnostic.labels[0].range, 0..10);
    }

    #[test]
    fn test_convert_parse_error_newlines_and_special_chars() {
        let span = Span { start: 0, end: 5 };
        
        let error1 = ParseError::DuplicateNames("name\nwith\nnewlines".to_string(), span);
        let diagnostic1 = convert_parse_error(0, &error1);
        assert_eq!(diagnostic1.message, "duplicate name: `name\nwith\nnewlines`");
        
        let error2 = ParseError::Selector("sel\tec\tor".to_string(), span);
        let diagnostic2 = convert_parse_error(0, &error2);
        assert_eq!(diagnostic2.message, "selector syntax is incorrect: sel\tec\tor");
    }

    // Note: Testing convert_pest_error and other functions that depend on external
    // pest types would require more complex setup and are better tested in integration tests
    // where we can create actual pest errors rather than mock them.
}
