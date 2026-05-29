//! daily-receipt CLI.
//!
//! Three subcommands:
//! * `render --summary <day.json> --content <content.json> --out <path>`
//! * `classify --summary <day.json>` — prints the day-type on stdout.
//! * `lint --summary <day.json> --content <content.json>` — validates
//!   without rendering.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use daily_receipt::{Content, DaySummary, RenderError, classify, lint, render};

mod cadence;

#[derive(Parser, Debug)]
#[command(version, about = "Deterministic ESC/POS renderer for the Daily Receipt", long_about = None)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Render one strip's ESC/POS bytes to a file.
    Render {
        /// Path to the day-summary JSON file.
        #[arg(long)]
        summary: PathBuf,
        /// Path to the content JSON file (haiku / glyph / stamp).
        #[arg(long)]
        content: PathBuf,
        /// Output path for the ESC/POS byte stream.
        #[arg(long)]
        out: PathBuf,
        /// Skip emitting a cadence `daily` record for this render.
        #[arg(long)]
        no_cadence_record: bool,
        /// Override the cadence record summary (default: derived from
        /// day-type plus repo/commit counts).
        #[arg(long)]
        cadence_summary: Option<String>,
    },
    /// Classify a day and print the day-type to stdout.
    Classify {
        /// Path to the day-summary JSON file.
        #[arg(long)]
        summary: PathBuf,
    },
    /// Validate a (summary, content) pair without rendering.
    Lint {
        /// Path to the day-summary JSON file.
        #[arg(long)]
        summary: PathBuf,
        /// Path to the content JSON file.
        #[arg(long)]
        content: PathBuf,
    },
}

fn main() -> ExitCode {
    match Cli::parse().cmd {
        Cmd::Render {
            summary,
            content,
            out,
            no_cadence_record,
            cadence_summary,
        } => match run_render(&summary, &content, &out, no_cadence_record, cadence_summary) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => e.into_exit(),
        },
        Cmd::Classify { summary } => match run_classify(&summary) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => e.into_exit(),
        },
        Cmd::Lint { summary, content } => match run_lint(&summary, &content) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => e.into_exit(),
        },
    }
}

enum CliError {
    Io(String, std::io::Error),
    Json(String, serde_json::Error),
    Render(RenderError),
}

impl CliError {
    fn into_exit(self) -> ExitCode {
        match &self {
            Self::Io(ctx, e) => {
                let mut stderr = std::io::stderr().lock();
                let _ = writeln!(stderr, "daily-receipt: io error ({ctx}): {e}");
                ExitCode::from(6)
            }
            Self::Json(ctx, e) => {
                let mut stderr = std::io::stderr().lock();
                let _ = writeln!(stderr, "daily-receipt: json error ({ctx}): {e}");
                ExitCode::from(7)
            }
            Self::Render(e) => {
                let mut stderr = std::io::stderr().lock();
                let _ = writeln!(stderr, "daily-receipt: render error: {e}");
                ExitCode::from(e.exit_code())
            }
        }
    }
}

impl From<RenderError> for CliError {
    fn from(value: RenderError) -> Self {
        Self::Render(value)
    }
}

fn read_summary(path: &PathBuf) -> Result<DaySummary, CliError> {
    let raw = fs::read_to_string(path)
        .map_err(|e| CliError::Io(format!("read summary {}", path.display()), e))?;
    serde_json::from_str::<DaySummary>(&raw)
        .map_err(|e| CliError::Json(format!("parse summary {}", path.display()), e))
}

fn read_content(path: &PathBuf) -> Result<Content, CliError> {
    let raw = fs::read_to_string(path)
        .map_err(|e| CliError::Io(format!("read content {}", path.display()), e))?;
    serde_json::from_str::<Content>(&raw)
        .map_err(|e| CliError::Json(format!("parse content {}", path.display()), e))
}

fn run_render(
    summary: &PathBuf,
    content: &PathBuf,
    out: &PathBuf,
    no_cadence_record: bool,
    cadence_summary: Option<String>,
) -> Result<(), CliError> {
    let summary = read_summary(summary)?;
    let content = read_content(content)?;
    let bytes = render(&summary, &content)?;
    fs::write(out, &bytes)
        .map_err(|e| CliError::Io(format!("write output {}", out.display()), e))?;
    // Register the emit in the cadence substrate (side effect; never blocks
    // the byte-stable render output). Opt out with `--no-cadence-record`.
    if !no_cadence_record {
        let day_type = classify(&summary);
        let text = cadence_summary.unwrap_or_else(|| cadence::derive_summary(&summary, day_type));
        cadence::record_emit(out, &text);
    }
    Ok(())
}

fn run_classify(summary: &PathBuf) -> Result<(), CliError> {
    let summary = read_summary(summary)?;
    let dt = classify(&summary);
    let mut stdout = std::io::stdout().lock();
    let line = match dt {
        daily_receipt::DayType::Workday => "workday",
        daily_receipt::DayType::Quiet => "quiet",
        daily_receipt::DayType::Special => "special",
    };
    writeln!(stdout, "{line}")
        .map_err(|e| CliError::Io("write classify result".into(), e))?;
    Ok(())
}

fn run_lint(summary: &PathBuf, content: &PathBuf) -> Result<(), CliError> {
    let summary = read_summary(summary)?;
    let content = read_content(content)?;
    let dt = lint(&summary, &content)?;
    let mut stdout = std::io::stdout().lock();
    let line = match dt {
        daily_receipt::DayType::Workday => "workday",
        daily_receipt::DayType::Quiet => "quiet",
        daily_receipt::DayType::Special => "special",
    };
    writeln!(stdout, "ok {line}")
        .map_err(|e| CliError::Io("write lint result".into(), e))?;
    Ok(())
}
