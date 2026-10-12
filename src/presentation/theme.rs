pub const CYAN: &str = "#5EF1D5";
pub const MINT: &str = "#42DDBB";
pub const TEAL: &str = "#80A6A2";
pub const AMBER: &str = "#FFCB78";
pub const CORAL: &str = "#FF738A";
pub const VIOLET: &str = "#B9A8FF";

pub const ANSI_CYAN: &str = "\x1b[38;2;94;241;213m";
pub const ANSI_MINT: &str = "\x1b[38;2;66;221;187m";
pub const ANSI_TEAL: &str = "\x1b[38;2;128;166;162m";
pub const ANSI_AMBER: &str = "\x1b[38;2;255;203;120m";
pub const ANSI_CORAL: &str = "\x1b[38;2;255;115;138m";
pub const ANSI_VIOLET: &str = "\x1b[38;2;185;168;255m";
pub const ANSI_RESET: &str = "\x1b[0m";
pub const ANSI_BOLD: &str = "\x1b[1m";
pub const ANSI_DIM: &str = "\x1b[2m";

pub fn paint(text: &str, ansi: &str, color_enabled: bool) -> String {
    if color_enabled {
        format!("{}{}{}", ansi, text, ANSI_RESET)
    } else {
        text.to_string()
    }
}
