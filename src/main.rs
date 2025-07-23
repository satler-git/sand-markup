use anyhow::Result;

use sand::parser::Document;

use std::path::{Path, PathBuf};
use tokio::{fs::File, io::AsyncReadExt};

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
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
}

fn convert_to_doc_displaying_errs(input: &str, path: &Path) -> Document {
    use codespan_reporting::{
        files::SimpleFiles,
        term::{Config, emit, termcolor},
    };
    use pest::Parser as _;
    use sand::parser::{Rule, SandParser, convert_parse_error, convert_pest_error};

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
    }

    Ok(())
}
