//! `nami completions <shell>`: emit a shell-completion script for the
//! current CLI surface to stdout.
//!
//! Implementation is a thin wrapper over `clap_complete::generate`,
//! which derives the script from the live `clap` Command tree — so
//! completions stay in sync with the CLI surface automatically (new
//! subcommands, new flags, new value enums all flow through without
//! changes here).
//!
//! Usage:
//!
//! ```sh
//! nami completions bash > ~/.local/share/bash-completion/completions/nami
//! nami completions zsh  > ~/.local/share/zsh/site-functions/_nami
//! nami completions fish > ~/.config/fish/completions/nami.fish
//! ```

use std::io::Write;

use anyhow::Result;
use clap::CommandFactory;

use crate::{Cli, CompletionsArgs};

pub fn run(args: CompletionsArgs) -> Result<()> {
    generate_to(args.shell, &mut std::io::stdout())
}

/// Write the completion script for `shell` into `out`. Pure with
/// respect to the runtime environment so tests can capture and
/// assert on the generated content.
fn generate_to(shell: clap_complete::Shell, out: &mut impl Write) -> Result<()> {
    let mut cmd = Cli::command();
    clap_complete::generate(shell, &mut cmd, "nami", out);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap_complete::Shell;

    fn generated(shell: Shell) -> String {
        let mut buf: Vec<u8> = Vec::new();
        generate_to(shell, &mut buf).unwrap();
        String::from_utf8(buf).expect("completion script must be valid UTF-8")
    }

    /// The generated script for each shell must mention the binary
    /// name and the current subcommand surface — that's the only
    /// stable contract clap_complete exposes, and it's what guards
    /// against accidentally shipping a stale script.
    fn assert_mentions_subcommands(script: &str) {
        for sub in [
            "run",
            "preview",
            "refresh",
            "forecast",
            "status",
            "init",
            "doctor",
            "completions",
        ] {
            assert!(
                script.contains(sub),
                "completion script missing subcommand {sub:?}"
            );
        }
    }

    #[test]
    fn bash_script_contains_binary_and_subcommands() {
        let script = generated(Shell::Bash);
        assert!(script.contains("nami"));
        // bash completion entry point is a `_nami()` function.
        assert!(script.contains("_nami"));
        assert_mentions_subcommands(&script);
    }

    #[test]
    fn zsh_script_contains_binary_and_subcommands() {
        let script = generated(Shell::Zsh);
        assert!(script.contains("#compdef nami"));
        assert_mentions_subcommands(&script);
    }

    #[test]
    fn fish_script_contains_binary_and_subcommands() {
        let script = generated(Shell::Fish);
        assert!(script.contains("complete -c nami"));
        assert_mentions_subcommands(&script);
    }
}
