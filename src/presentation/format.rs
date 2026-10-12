use std::io::IsTerminal;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum OutputFormat {
    #[default]
    Auto,
    Rich,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputMode {
    pub rich: bool,
    pub color: bool,
}

pub fn color_enabled() -> bool {
    if std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
        return false;
    }
    match std::env::var("TERM") {
        Ok(t) if t == "dumb" => return false,
        _ => {}
    }
    std::io::stdout().is_terminal()
}

pub fn resolve_mode(format: OutputFormat) -> OutputMode {
    match format {
        OutputFormat::Json => OutputMode {
            rich: false,
            color: false,
        },
        OutputFormat::Rich => OutputMode {
            rich: true,
            color: color_enabled(),
        },
        OutputFormat::Auto => {
            if std::io::stdout().is_terminal() {
                OutputMode {
                    rich: true,
                    color: color_enabled(),
                }
            } else {
                OutputMode {
                    rich: false,
                    color: false,
                }
            }
        }
    }
}

pub fn resolve_mode_for_test(
    format: OutputFormat,
    tty: bool,
    no_color: bool,
    dumb: bool,
) -> OutputMode {
    let color = !no_color && !dumb && tty;
    match format {
        OutputFormat::Json => OutputMode {
            rich: false,
            color: false,
        },
        OutputFormat::Rich => OutputMode { rich: true, color },
        OutputFormat::Auto => {
            if tty {
                OutputMode { rich: true, color }
            } else {
                OutputMode {
                    rich: false,
                    color: false,
                }
            }
        }
    }
}
