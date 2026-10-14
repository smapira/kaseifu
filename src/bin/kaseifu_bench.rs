//! `kaseifu-bench`: the TREC benchmark research interface.
//!
//! Separate from `kaseifu` on purpose (see `kaseifu::benchmark::cli`). The
//! whole implementation lives in the library so it is testable without a
//! process boundary; this file only forwards the exit code.

fn main() -> std::process::ExitCode {
    kaseifu::benchmark::cli::main()
}
