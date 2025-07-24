use anyhow::Result;

use sand::parser::{Document, ParseError, Span};

use std::path::{Path, PathBuf};
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
    /// Parse and validate it
    Parse {
        #[arg(value_name = "FILE", value_parser)]
        input: PathBuf,
    },
    Lsp,

    Completions {
        #[arg(long)]
        shell: clap_complete::Shell,
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

fn convert_to_doc_displaying_errs(input: &str, path: &Path) -> Document {
    use codespan_reporting::{
        files::SimpleFiles,
        term::{Config, emit, termcolor},
    };
    use pest::Parser as _;
    use sand::parser::{Rule, SandParser};

    let pairs = SandParser::parse(Rule::doc, input);

    let mut files = SimpleFiles::new();

    let filename = path.display().to_string();
    let file_id = files.add(filename, input.to_string());

    let writer = termcolor::StandardStream::stderr(termcolor::ColorChoice::Auto);
    let config = Config::default();

    match pairs {
        Err(parsing_error) => {
            let diag = convert_pest_error(file_id, parsing_error);
            emit(&mut writer.lock(), &config, &files, &diag).unwrap();
            std::process::exit(1)
        }
        Ok(pairs) => {
            let doc: Result<Document, _> = pairs.try_into();
            match doc {
                Ok(doc) => doc,
                Err(errs) => {
                    for err in errs {
                        let diag = convert_parse_error(file_id, &err);
                        emit(&mut writer.lock(), &config, &files, &diag).unwrap();
                    }
                    std::process::exit(1)
                }
            }
        }
    }
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

            let doc = convert_to_doc_displaying_errs(&contents, &input);
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
    }

    Ok(())
}
