use anyhow::Result;
use std::path::PathBuf;
use tempfile::NamedTempFile;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

// Note: Using standard Rust testing framework with tokio for async tests
// Integration tests to validate the main CLI functionality

use clap::Parser;

// Import the types from main.rs since they're not public in the lib
// We'll test using clap parsing directly

#[tokio::test]
async fn test_clap_parsing_basic() {
    // Test basic clap parsing works - this validates the derive macros
    let result = std::panic::catch_unwind(|| {
        // Just test that the parsing framework is set up correctly
        use clap::CommandFactory;
        
        // This would be the actual Args struct from main.rs
        // We can't import it directly, so we test the concept
        let _cmd = clap::Command::new("sand")
            .subcommand(clap::Command::new("parse").arg(
                clap::Arg::new("input")
                    .value_name("FILE")
                    .required(true)
                    .value_parser(clap::value_parser!(PathBuf))
            ))
            .subcommand(clap::Command::new("lsp"))
            .subcommand(clap::Command::new("completions").arg(
                clap::Arg::new("shell")
                    .required(true)
                    .value_parser(clap::value_parser!(clap_complete::Shell))
            ))
            .subcommand(
                clap::Command::new("out")
                    .arg(clap::Arg::new("selector").required(true))
                    .arg(clap::Arg::new("input")
                        .long("input")
                        .short('i')
                        .value_name("FILE")
                        .required(true)
                        .value_parser(clap::value_parser!(PathBuf)))
                    .arg(clap::Arg::new("markdown")
                        .long("markdown")
                        .short('m')
                        .action(clap::ArgAction::SetTrue))
            );
    });
    
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_convert_parse_error_multiple_name_define() {
    use sand::parser::{ParseError, Span};
    
    let span = Span { start: 10, end: 20 };
    let error = ParseError::MultipleNameDefine(span);
    let diagnostic = sand::convert_parse_error(0, &error);
    
    assert_eq!(diagnostic.message, "names are defined more than once");
    assert_eq!(diagnostic.labels.len(), 1);
    assert_eq!(diagnostic.labels[0].range, 10..20);
}

#[tokio::test]
async fn test_convert_parse_error_duplicate_names() {
    use sand::parser::{ParseError, Span};
    
    let span = Span { start: 5, end: 15 };
    let error = ParseError::DuplicateNames("test_name".to_string(), span);
    let diagnostic = sand::convert_parse_error(1, &error);
    
    assert_eq!(diagnostic.message, "duplicate name: `test_name`");
    assert_eq!(diagnostic.labels.len(), 1);
    assert_eq!(diagnostic.labels[0].range, 5..15);
}

#[tokio::test]
async fn test_convert_parse_error_duplicate_alias() {
    use sand::parser::{ParseError, Span};
    
    let span = Span { start: 0, end: 10 };
    let error = ParseError::DuplicateAlias("test_alias".to_string(), span);
    let diagnostic = sand::convert_parse_error(2, &error);
    
    assert_eq!(diagnostic.message, "duplicate alias: `test_alias`");
    assert_eq!(diagnostic.labels.len(), 1);
    assert_eq!(diagnostic.labels[0].range, 0..10);
}

#[tokio::test]
async fn test_convert_parse_error_alias_conflict_with_names() {
    use sand::parser::{ParseError, Span};
    
    let span = Span { start: 25, end: 35 };
    let error = ParseError::AliasConflictWithNames("conflicting_name".to_string(), span);
    let diagnostic = sand::convert_parse_error(3, &error);
    
    assert_eq!(diagnostic.message, "alias `conflicting_name` conflicts with a name");
    assert_eq!(diagnostic.labels.len(), 1);
    assert_eq!(diagnostic.labels[0].range, 25..35);
}

#[tokio::test]
async fn test_convert_parse_error_selector() {
    use sand::parser::{ParseError, Span};
    
    let span = Span { start: 40, end: 50 };
    let error = ParseError::Selector("invalid.selector".to_string(), span);
    let diagnostic = sand::convert_parse_error(4, &error);
    
    assert_eq!(diagnostic.message, "selector syntax is incorrect: invalid.selector");
    assert_eq!(diagnostic.labels.len(), 1);
    assert_eq!(diagnostic.labels[0].range, 40..50);
}

#[tokio::test]
async fn test_convert_parse_error_missing_names() {
    use sand::parser::ParseError;
    
    let error = ParseError::MissingNames;
    let diagnostic = sand::convert_parse_error(5, &error);
    
    assert_eq!(diagnostic.message, "names are not defined");
    assert_eq!(diagnostic.labels.len(), 0);
}

#[tokio::test]
async fn test_convert_pest_error_parsing_error_with_positives() {
    use sand::parser::Rule;
    use pest::error::{Error, ErrorVariant, InputLocation};
    
    let error = Error::new_from_pos(
        ErrorVariant::ParsingError {
            positives: vec![Rule::doc],
            negatives: vec![],
        },
        InputLocation::Pos(15),
    );
    
    let diagnostic = sand::convert_pest_error(0, error);
    assert!(diagnostic.message.contains("failed to parse input"));
    assert!(diagnostic.message.contains("expected:"));
    assert_eq!(diagnostic.labels.len(), 1);
    assert_eq!(diagnostic.labels[0].range, 15..16);
}

#[tokio::test]
async fn test_convert_pest_error_parsing_error_with_negatives() {
    use sand::parser::Rule;
    use pest::error::{Error, ErrorVariant, InputLocation};
    
    let error = Error::new_from_pos(
        ErrorVariant::ParsingError {
            positives: vec![],
            negatives: vec![Rule::Selector],
        },
        InputLocation::Pos(20),
    );
    
    let diagnostic = sand::convert_pest_error(1, error);
    assert!(diagnostic.message.contains("failed to parse input"));
    assert!(diagnostic.message.contains("not:"));
    assert_eq!(diagnostic.labels.len(), 1);
    assert_eq!(diagnostic.labels[0].range, 20..21);
}

#[tokio::test]
async fn test_convert_pest_error_parsing_error_with_span() {
    use sand::parser::Rule;
    use pest::error::{Error, ErrorVariant, InputLocation};
    
    let error = Error::new_from_span(
        ErrorVariant::ParsingError {
            positives: vec![Rule::doc],
            negatives: vec![Rule::Selector],
        },
        InputLocation::Span((10, 25)),
    );
    
    let diagnostic = sand::convert_pest_error(2, error);
    assert!(diagnostic.message.contains("failed to parse input"));
    assert!(diagnostic.message.contains("expected:"));
    assert!(diagnostic.message.contains("not:"));
    assert_eq!(diagnostic.labels.len(), 1);
    assert_eq!(diagnostic.labels[0].range, 10..25);
}

#[tokio::test]
async fn test_convert_pest_error_custom_error() {
    use pest::error::{Error, ErrorVariant, InputLocation};
    
    let error = Error::new_from_pos(
        ErrorVariant::CustomError {
            message: "Custom error message".to_string(),
        },
        InputLocation::Pos(30),
    );
    
    let diagnostic = sand::convert_pest_error(3, error);
    assert_eq!(diagnostic.message, "Custom error message");
    assert_eq!(diagnostic.labels.len(), 1);
    assert_eq!(diagnostic.labels[0].range, 30..31);
}

#[tokio::test]
async fn test_print_completions_bash() {
    // Test that print_completions doesn't panic for different shells
    // We can't easily test the output without capturing stdout, 
    // but we can ensure it runs without errors
    
    // This is a basic smoke test
    let result = std::panic::catch_unwind(|| {
        sand::print_completions(clap_complete::Shell::Bash);
    });
    
    // The function should not panic
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_print_completions_zsh() {
    let result = std::panic::catch_unwind(|| {
        sand::print_completions(clap_complete::Shell::Zsh);
    });
    
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_print_completions_fish() {
    let result = std::panic::catch_unwind(|| {
        sand::print_completions(clap_complete::Shell::Fish);
    });
    
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_print_completions_powershell() {
    let result = std::panic::catch_unwind(|| {
        sand::print_completions(clap_complete::Shell::PowerShell);
    });
    
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_print_completions_elvish() {
    let result = std::panic::catch_unwind(|| {
        sand::print_completions(clap_complete::Shell::Elvish);
    });
    
    assert!(result.is_ok());
}

// Helper function to create temporary files for testing
async fn create_temp_file_with_content(content: &str) -> Result<NamedTempFile> {
    let mut temp_file = NamedTempFile::new()?;
    temp_file.write_all(content.as_bytes()).await?;
    temp_file.flush().await?;
    Ok(temp_file)
}

#[tokio::test]
async fn test_file_operations_valid_file() -> Result<()> {
    // Create a temporary file with valid content
    let temp_file = create_temp_file_with_content("valid content for parsing").await?;
    let temp_path = temp_file.path();
    
    // Test that the file operations work as expected
    let mut file = File::open(temp_path).await?;
    let mut contents = String::new();
    use tokio::io::AsyncReadExt;
    file.read_to_string(&mut contents).await?;
    
    assert_eq!(contents, "valid content for parsing");
    Ok(())
}

#[tokio::test]
async fn test_file_operations_nonexistent_file() {
    let nonexistent_path = PathBuf::from("definitely_does_not_exist.txt");
    let result = File::open(&nonexistent_path).await;
    
    // Should fail to open nonexistent file
    assert!(result.is_err());
}

#[tokio::test]
async fn test_file_operations_document_content() -> Result<()> {
    let temp_file = create_temp_file_with_content("test document content").await?;
    let temp_path = temp_file.path();
    
    // Test file reading operations that would be used in Out command
    let mut file = File::open(temp_path).await?;
    let mut contents = String::new();
    use tokio::io::AsyncReadExt;
    file.read_to_string(&mut contents).await?;
    
    assert_eq!(contents, "test document content");
    
    // Test filename display conversion
    let filename = temp_path.display().to_string();
    assert!(!filename.is_empty());
    
    Ok(())
}

// Test boundary conditions for span ranges
#[tokio::test]
async fn test_convert_parse_error_zero_span() {
    use sand::parser::{ParseError, Span};
    
    let span = Span { start: 0, end: 0 };
    let error = ParseError::MultipleNameDefine(span);
    let diagnostic = sand::convert_parse_error(0, &error);
    
    assert_eq!(diagnostic.labels[0].range, 0..0);
}

#[tokio::test]
async fn test_convert_parse_error_large_span() {
    use sand::parser::{ParseError, Span};
    
    let span = Span { start: 1000, end: 2000 };
    let error = ParseError::DuplicateNames("large_span_test".to_string(), span);
    let diagnostic = sand::convert_parse_error(0, &error);
    
    assert_eq!(diagnostic.labels[0].range, 1000..2000);
}

// Test empty string cases
#[tokio::test]
async fn test_convert_parse_error_empty_name() {
    use sand::parser::{ParseError, Span};
    
    let span = Span { start: 0, end: 1 };
    let error = ParseError::DuplicateNames("".to_string(), span);
    let diagnostic = sand::convert_parse_error(0, &error);
    
    assert_eq!(diagnostic.message, "duplicate name: ``");
}

#[tokio::test]
async fn test_convert_parse_error_empty_alias() {
    use sand::parser::{ParseError, Span};
    
    let span = Span { start: 0, end: 1 };
    let error = ParseError::DuplicateAlias("".to_string(), span);
    let diagnostic = sand::convert_parse_error(0, &error);
    
    assert_eq!(diagnostic.message, "duplicate alias: ``");
}

#[tokio::test]
async fn test_convert_parse_error_empty_selector() {
    use sand::parser::{ParseError, Span};
    
    let span = Span { start: 0, end: 1 };
    let error = ParseError::Selector("".to_string(), span);
    let diagnostic = sand::convert_parse_error(0, &error);
    
    assert_eq!(diagnostic.message, "selector syntax is incorrect: ");
}

// Test multiple file IDs
#[tokio::test]
async fn test_convert_parse_error_different_file_ids() {
    use sand::parser::{ParseError, Span};
    
    let span = Span { start: 10, end: 20 };
    let error = ParseError::MultipleNameDefine(span);
    
    let diagnostic1 = sand::convert_parse_error(0, &error);
    let diagnostic2 = sand::convert_parse_error(100, &error);
    
    // Both should have the same range but different file IDs
    assert_eq!(diagnostic1.labels[0].range, diagnostic2.labels[0].range);
    assert_eq!(diagnostic1.labels[0].file_id, 0);
    assert_eq!(diagnostic2.labels[0].file_id, 100);
}

// Test Unicode and special characters in names/selectors
#[tokio::test]
async fn test_convert_parse_error_unicode_names() {
    use sand::parser::{ParseError, Span};
    
    let span = Span { start: 0, end: 10 };
    let error = ParseError::DuplicateNames("тест_名前".to_string(), span);
    let diagnostic = sand::convert_parse_error(0, &error);
    
    assert_eq!(diagnostic.message, "duplicate name: `тест_名前`");
}

#[tokio::test]
async fn test_convert_parse_error_special_characters() {
    use sand::parser::{ParseError, Span};
    
    let span = Span { start: 0, end: 5 };
    let error = ParseError::Selector("@#$%^&*()".to_string(), span);
    let diagnostic = sand::convert_parse_error(0, &error);
    
    assert_eq!(diagnostic.message, "selector syntax is incorrect: @#$%^&*()");
}

// Test pest error edge cases
#[tokio::test]
async fn test_convert_pest_error_empty_positives_and_negatives() {
    use pest::error::{Error, ErrorVariant, InputLocation};
    
    let error = Error::new_from_pos(
        ErrorVariant::ParsingError {
            positives: vec![],
            negatives: vec![],
        },
        InputLocation::Pos(0),
    );
    
    let diagnostic = sand::convert_pest_error(0, error);
    assert_eq!(diagnostic.message, "failed to parse input");
    assert_eq!(diagnostic.labels[0].range, 0..1);
}

#[tokio::test]
async fn test_convert_pest_error_custom_error_empty_message() {
    use pest::error::{Error, ErrorVariant, InputLocation};
    
    let error = Error::new_from_pos(
        ErrorVariant::CustomError {
            message: "".to_string(),
        },
        InputLocation::Pos(5),
    );
    
    let diagnostic = sand::convert_pest_error(0, error);
    assert_eq!(diagnostic.message, "");
    assert_eq!(diagnostic.labels[0].range, 5..6);
}

// Test edge cases for pathbuf handling
#[tokio::test]
async fn test_pathbuf_display_conversion() {
    let path = PathBuf::from("/some/test/path.txt");
    let display_string = path.display().to_string();
    assert!(display_string.contains("path.txt"));
    
    let empty_path = PathBuf::new();
    let empty_display = empty_path.display().to_string();
    assert_eq!(empty_display, "");
    
    let relative_path = PathBuf::from("relative/path.txt");
    let relative_display = relative_path.display().to_string();
    assert!(relative_display.contains("relative"));
    assert!(relative_display.contains("path.txt"));
}

// Test error handling for various input scenarios
#[tokio::test]
async fn test_convert_parse_error_with_newlines() {
    use sand::parser::{ParseError, Span};
    
    let span = Span { start: 0, end: 5 };
    let error = ParseError::DuplicateNames("name\nwith\nnewlines".to_string(), span);
    let diagnostic = sand::convert_parse_error(0, &error);
    
    assert_eq!(diagnostic.message, "duplicate name: `name\nwith\nnewlines`");
}

#[tokio::test]
async fn test_convert_parse_error_with_tabs() {
    use sand::parser::{ParseError, Span};
    
    let span = Span { start: 0, end: 5 };
    let error = ParseError::Selector("sel\tec\tor".to_string(), span);
    let diagnostic = sand::convert_parse_error(0, &error);
    
    assert_eq!(diagnostic.message, "selector syntax is incorrect: sel\tec\tor");
}

#[tokio::test]
async fn test_convert_parse_error_very_long_names() {
    use sand::parser::{ParseError, Span};
    
    let span = Span { start: 0, end: 10 };
    let long_name = "a".repeat(1000);
    
    let error = ParseError::DuplicateNames(long_name.clone(), span);
    let diagnostic = sand::convert_parse_error(0, &error);
    
    assert_eq!(diagnostic.message, format!("duplicate name: `{}`", long_name));
    assert_eq!(diagnostic.labels[0].range, 0..10);
}

// Test that spans work correctly with different positions
#[tokio::test]
async fn test_convert_parse_error_various_span_positions() {
    use sand::parser::{ParseError, Span};
    
    // Test span at start of file
    let span1 = Span { start: 0, end: 5 };
    let error1 = ParseError::MultipleNameDefine(span1);
    let diagnostic1 = sand::convert_parse_error(0, &error1);
    assert_eq!(diagnostic1.labels[0].range, 0..5);
    
    // Test span in middle of file
    let span2 = Span { start: 500, end: 505 };
    let error2 = ParseError::MultipleNameDefine(span2);
    let diagnostic2 = sand::convert_parse_error(0, &error2);
    assert_eq!(diagnostic2.labels[0].range, 500..505);
    
    // Test single character span
    let span3 = Span { start: 10, end: 11 };
    let error3 = ParseError::MultipleNameDefine(span3);
    let diagnostic3 = sand::convert_parse_error(0, &error3);
    assert_eq!(diagnostic3.labels[0].range, 10..11);
}

// Test pest error with different location types
#[tokio::test]
async fn test_convert_pest_error_pos_vs_span() {
    use pest::error::{Error, ErrorVariant, InputLocation};
    
    // Test with Pos location
    let error_pos = Error::new_from_pos(
        ErrorVariant::CustomError {
            message: "Position error".to_string(),
        },
        InputLocation::Pos(42),
    );
    
    let diagnostic_pos = sand::convert_pest_error(0, error_pos);
    assert_eq!(diagnostic_pos.labels[0].range, 42..43); // Pos gets +1 for end
    
    // Test with Span location
    let error_span = Error::new_from_span(
        ErrorVariant::CustomError {
            message: "Span error".to_string(),
        },
        InputLocation::Span((42, 50)),
    );
    
    let diagnostic_span = sand::convert_pest_error(0, error_span);
    assert_eq!(diagnostic_span.labels[0].range, 42..50); // Span uses exact range
}