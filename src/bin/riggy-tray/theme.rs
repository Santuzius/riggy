//! Whether the desktop uses a dark colour scheme, from the freedesktop settings portal (`org.freedesktop.appearance color-scheme`: 1 dark, 2 light, 0 no preference), read with `busctl`.

use std::process::Command;

/// True for a dark scheme; a light one, no preference or no portal count as light, since a dark icon on a light panel is the safer guess.
pub fn dark_scheme() -> bool {
    let output = Command::new("busctl")
        .args(["--user", "call", "org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop", "org.freedesktop.portal.Settings", "ReadOne", "ss", "org.freedesktop.appearance", "color-scheme"])
        .output();
    output.is_ok_and(|output| scheme_is_dark(&String::from_utf8_lossy(&output.stdout)))
}

/// Reads busctl's answer, e.g. `v u 1`.
fn scheme_is_dark(answer: &str) -> bool {
    answer.split_whitespace().last() == Some("1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_scheme_one_is_dark() {
        assert!(scheme_is_dark("v u 1\n"));
        assert!(!scheme_is_dark("v u 2\n"));
        assert!(!scheme_is_dark(""));
    }
}
