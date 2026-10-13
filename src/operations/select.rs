//! Interactive destination selection (ZC-13).
//!
//! The human picks the destination from the candidates Discovery found; Jev
//! is not consulted, so this flow needs no API key, no Keychain access and
//! no network. The module contains the pure pieces (answer parsing) so the
//! interactive loop in the CLI stays thin and testable.

/// Prompt shown while waiting for a selection answer.
pub const SELECT_PROMPT: &str = "Select a destination number (or q to cancel)";

/// One parsed answer to the selection prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectCommand {
    /// 0-based index into the displayed candidate list.
    Choose(usize),
    /// Cancel: nothing is changed.
    Quit,
}

/// Parse one answer: `q` (any case) cancels, `1..=count` chooses.
///
/// Anything else is an error message for the caller to re-prompt with; an
/// invalid answer never selects a candidate and never cancels by accident.
pub fn parse_select_command(input: &str, count: usize) -> Result<SelectCommand, String> {
    let answer = input.trim();
    if answer.eq_ignore_ascii_case("q") {
        return Ok(SelectCommand::Quit);
    }
    match answer.parse::<usize>() {
        Ok(number) if (1..=count).contains(&number) => Ok(SelectCommand::Choose(number - 1)),
        _ => Err(format!("expected a number from 1 to {count} (or q)")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_select_by_position() {
        assert_eq!(parse_select_command("1", 3), Ok(SelectCommand::Choose(0)));
        assert_eq!(parse_select_command(" 2 ", 3), Ok(SelectCommand::Choose(1)));
        assert_eq!(parse_select_command("3", 3), Ok(SelectCommand::Choose(2)));
    }

    #[test]
    fn q_cancels_in_any_case() {
        assert_eq!(parse_select_command("q", 3), Ok(SelectCommand::Quit));
        assert_eq!(parse_select_command("Q", 3), Ok(SelectCommand::Quit));
        assert_eq!(parse_select_command(" q ", 3), Ok(SelectCommand::Quit));
    }

    #[test]
    fn invalid_answers_are_rejected_with_the_range() {
        for answer in ["0", "4", "", "two", "-1", "1.5"] {
            let error = parse_select_command(answer, 3).expect_err(answer);
            assert!(error.contains("1 to 3"), "{error}");
        }
    }
}
