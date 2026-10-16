use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use clap::{Parser, Subcommand};
use color_eyre::Result;

use crate::windows_settings::WindowsSettings;

#[derive(Parser)]
#[command(version, about = "Local hotkey dictation (Windows preview)")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run hotkey dictation and automatic paste until interrupted (the default).
    Dictate {
        /// Select an input device by a case-insensitive name fragment.
        #[arg(long)]
        device: Option<String>,
    },
    /// List microphone devices visible through WASAPI.
    Devices,
    /// Print the most recent dictation observations.
    Status {
        /// Maximum number of observations to print.
        #[arg(long, default_value_t = 20)]
        lines: usize,
    },
    /// Write default settings if none exist, then print their path and contents.
    Settings,
    /// Install, inspect, and validate the local dictation model.
    Model {
        #[command(subcommand)]
        command: ModelCommand,
    },
}

#[derive(Subcommand)]
enum ModelCommand {
    /// Show whether the default dictation model is installed.
    Status,
    /// Download and checksum-verify the default dictation model.
    Install,
    /// List transcribe.cpp compute devices available to HEX.
    Devices,
    /// Load and prewarm the installed model.
    Check,
    /// Transcribe a WAV file with the installed model.
    Transcribe { wav: PathBuf },
}

pub fn run(shutdown: &'static AtomicBool) -> Result<()> {
    color_eyre::install()?;
    let log_dir = crate::app_paths::init_process_logging(shutdown)?;
    let event_path = log_dir.join("live.ndjson");
    match Cli::parse()
        .command
        .unwrap_or(Command::Dictate { device: None })
    {
        Command::Dictate { device } => {
            let _instance = crate::instance::acquire("listener")?;
            let settings = WindowsSettings::load()?;
            crate::windows_dictation::initialize_feedback(settings.sound_effect_volume);
            crate::windows_dictation::run(&event_path, device.as_deref(), shutdown, settings, None)
        }
        Command::Devices => {
            for device in crate::audio::input_device_names()? {
                println!("{device}");
            }
            Ok(())
        }
        Command::Status { lines } => print_status(&event_path, lines),
        Command::Settings => {
            let settings = WindowsSettings::load()?;
            settings.save()?;
            println!("{}", WindowsSettings::path()?.display());
            println!("{}", serde_json::to_string_pretty(&settings)?);
            Ok(())
        }
        Command::Model { command } => match command {
            ModelCommand::Status => {
                let model = crate::linux_transcriber::default_model();
                println!(
                    "{}\t{}\t{}",
                    if crate::transcription_models::is_installed(model, "en") {
                        "installed"
                    } else {
                        "missing"
                    },
                    model.name,
                    crate::transcription_models::model_path(model)?.display()
                );
                Ok(())
            }
            ModelCommand::Install => crate::linux_transcriber::install_default(shutdown),
            ModelCommand::Devices => {
                for device in crate::linux_transcriber::devices()? {
                    println!("{device}");
                }
                Ok(())
            }
            ModelCommand::Check => {
                let transcriber = crate::linux_transcriber::LinuxTranscriber::load_default()?;
                println!("ready\t{}", transcriber.device_label());
                Ok(())
            }
            ModelCommand::Transcribe { wav } => {
                println!("{}", crate::linux_transcriber::transcribe_wav(&wav)?);
                Ok(())
            }
        },
    }
}

fn print_status(path: &Path, limit: usize) -> Result<()> {
    if !path.exists() {
        println!("No dictation observations yet at {}", path.display());
        return Ok(());
    }
    let lines = BufReader::new(std::fs::File::open(path)?)
        .lines()
        .collect::<std::io::Result<Vec<_>>>()?;
    for line in lines.iter().skip(lines.len().saturating_sub(limit)) {
        println!("{line}");
    }
    Ok(())
}
