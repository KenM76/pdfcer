//! Exit quietly when the reader of stdout has gone away.
//!
//! `println!` panics when a write to stdout fails. The common cause is a
//! pipeline whose consumer stopped reading early (`pdfcer ... | head -1`).
//! That is the operator's choice, not a defect. A panic message and backtrace
//! hint on stderr would read as a crash, so the hook below exits with status 0
//! instead. Every other panic goes to the default hook unchanged.

use std::panic::PanicHookInfo;

/// Install the hook. Call once, before any output is written.
pub(crate) fn install() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info: &PanicHookInfo<'_>| {
        if payload_text(info).is_some_and(is_closed_stdout) {
            std::process::exit(0);
        }
        default(info);
    }));
}

fn payload_text<'a>(info: &'a PanicHookInfo<'_>) -> Option<&'a str> {
    let p = info.payload();
    p.downcast_ref::<&str>()
        .copied()
        .or_else(|| p.downcast_ref::<String>().map(String::as_str))
}

/// Whether a panic message is std's stdout-write failure caused by a closed
/// pipe: `EPIPE` (32) on Unix, or `ERROR_BROKEN_PIPE` (109) or `ERROR_NO_DATA`
/// (232, "the pipe is being closed") on Windows. Each is what std maps to
/// `io::ErrorKind::BrokenPipe`.
fn is_closed_stdout(msg: &str) -> bool {
    msg.starts_with("failed printing to stdout")
        && [" (os error 32)", " (os error 109)", " (os error 232)"]
            .iter()
            .any(|code| msg.ends_with(code))
}

#[cfg(test)]
mod tests {
    use super::is_closed_stdout;

    #[test]
    fn a_closed_pipe_on_either_platform_is_recognised() {
        for m in [
            "failed printing to stdout: The pipe is being closed. (os error 232)",
            "failed printing to stdout: The pipe has been ended. (os error 109)",
            "failed printing to stdout: Broken pipe (os error 32)",
        ] {
            assert!(is_closed_stdout(m), "{m}");
        }
    }

    #[test]
    fn other_failures_still_panic() {
        for m in [
            "failed printing to stdout: Access is denied. (os error 5)",
            "failed printing to stderr: Broken pipe (os error 32)",
            "index out of bounds (os error 32)",
        ] {
            assert!(!is_closed_stdout(m), "{m}");
        }
    }
}
