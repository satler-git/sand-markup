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

    Out {
        selector: String,
        #[arg(long, short, value_name = "FILE", value_parser)]
        input: PathBuf,
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

fn print_completions<G: clap_complete::Generator>(g: G) {
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
        Command::Out { selector, input } => {
            let mut file = File::open(&input).await?;

            let mut contents = String::new();
            file.read_to_string(&mut contents).await?;

            let filename = input.display().to_string();
            let doc = convert_to_doc_displaying_errs(&contents, &filename);
            let sel = convert_to_sel_displaying_errs(&selector, &doc, "<user>");

            let rendered = sand::formatter::render_plain(&doc, &sel);
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
