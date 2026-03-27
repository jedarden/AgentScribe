//! Shell hook generation for auto-querying AgentScribe on command failure.
//!
//! Generates shell snippets that detect non-zero exit codes and trigger a
//! background AgentScribe search, printing a one-line hint on the next prompt.
//!
//! Usage:
//!   eval "$(agentscribe shell-hook bash)"   # in ~/.bashrc
//!   eval "$(agentscribe shell-hook zsh)"    # in ~/.zshrc
//!   agentscribe shell-hook fish | source    # in ~/.config/fish/config.fish

use crate::config::ShellHookConfig;
use crate::error::{AgentScribeError, Result};

/// Generate a shell integration snippet for the given shell.
pub fn generate_hook(shell: &str, config: &ShellHookConfig) -> Result<String> {
    let snippet = match shell {
        "bash" => bash_hook(config),
        "zsh" => zsh_hook(config),
        "fish" => fish_hook(config),
        other => {
            return Err(AgentScribeError::Config(format!(
                "unsupported shell '{}'; supported shells: bash, zsh, fish",
                other
            )))
        }
    };
    Ok(snippet)
}

fn bash_hook(config: &ShellHookConfig) -> String {
    if config.background {
        r#"# AgentScribe shell hook — auto-query on error (bash)
# Setup: eval "$(agentscribe shell-hook bash)" in ~/.bashrc

__agentscribe_hint_file=""

__agentscribe_prompt_cmd() {
    local __exit=$?
    # Print hint from previous failed command if the background search finished
    if [[ -n "$__agentscribe_hint_file" ]]; then
        local __f="$__agentscribe_hint_file"
        __agentscribe_hint_file=""
        if [[ -s "$__f" ]]; then
            printf '\033[33m[agentscribe] %s\033[0m\n' "$(< "$__f")" >&2
        fi
        rm -f "$__f" 2>/dev/null
    fi
    # On non-zero exit, start a background search for the failed command
    if [[ $__exit -ne 0 ]]; then
        local __cmd
        __cmd=$(fc -ln -1 2>/dev/null)
        __cmd="${__cmd#"${__cmd%%[! ]*}"}"
        if [[ -n "$__cmd" && "$__cmd" != agentscribe\ * ]]; then
            local __f
            __f=$(mktemp /tmp/as_hint.XXXXXX 2>/dev/null) || return 0
            __agentscribe_hint_file="$__f"
            ( agentscribe search "$__cmd" -n 1 --solution_only --hint 2>/dev/null > "$__f" ) &
            disown $! 2>/dev/null
        fi
    fi
}

PROMPT_COMMAND="__agentscribe_prompt_cmd${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
"#
        .to_string()
    } else {
        r#"# AgentScribe shell hook — auto-query on error (bash, blocking mode)
# Setup: eval "$(agentscribe shell-hook bash)" in ~/.bashrc

__agentscribe_prompt_cmd() {
    local __exit=$?
    if [[ $__exit -ne 0 ]]; then
        local __cmd
        __cmd=$(fc -ln -1 2>/dev/null)
        __cmd="${__cmd#"${__cmd%%[! ]*}"}"
        if [[ -n "$__cmd" && "$__cmd" != agentscribe\ * ]]; then
            local __hint
            __hint=$(agentscribe search "$__cmd" -n 1 --solution_only --hint 2>/dev/null)
            if [[ -n "$__hint" ]]; then
                printf '\033[33m[agentscribe] %s\033[0m\n' "$__hint" >&2
            fi
        fi
    fi
}

PROMPT_COMMAND="__agentscribe_prompt_cmd${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
"#
        .to_string()
    }
}

fn zsh_hook(config: &ShellHookConfig) -> String {
    if config.background {
        r#"# AgentScribe shell hook — auto-query on error (zsh)
# Setup: eval "$(agentscribe shell-hook zsh)" in ~/.zshrc

__agentscribe_hint_file=""
__agentscribe_last_cmd=""

__agentscribe_preexec() {
    __agentscribe_last_cmd="$1"
}

__agentscribe_precmd() {
    local __exit=$?
    # Print hint from previous failed command if the background search finished
    if [[ -n "$__agentscribe_hint_file" ]]; then
        local __f="$__agentscribe_hint_file"
        __agentscribe_hint_file=""
        if [[ -s "$__f" ]]; then
            printf '\033[33m[agentscribe] %s\033[0m\n' "$(< "$__f")" >&2
        fi
        rm -f "$__f" 2>/dev/null
    fi
    # On non-zero exit, start a background search for the failed command
    if [[ $__exit -ne 0 && -n "$__agentscribe_last_cmd" ]]; then
        local __cmd="$__agentscribe_last_cmd"
        if [[ "$__cmd" != agentscribe\ * ]]; then
            local __f
            __f=$(mktemp /tmp/as_hint.XXXXXX 2>/dev/null) || { __agentscribe_last_cmd=""; return 0; }
            __agentscribe_hint_file="$__f"
            ( agentscribe search "$__cmd" -n 1 --solution_only --hint 2>/dev/null > "$__f" ) &
            disown $! 2>/dev/null
        fi
    fi
    __agentscribe_last_cmd=""
}

autoload -Uz add-zsh-hook
add-zsh-hook preexec __agentscribe_preexec
add-zsh-hook precmd __agentscribe_precmd
"#
        .to_string()
    } else {
        r#"# AgentScribe shell hook — auto-query on error (zsh, blocking mode)
# Setup: eval "$(agentscribe shell-hook zsh)" in ~/.zshrc

__agentscribe_last_cmd=""

__agentscribe_preexec() {
    __agentscribe_last_cmd="$1"
}

__agentscribe_precmd() {
    local __exit=$?
    if [[ $__exit -ne 0 && -n "$__agentscribe_last_cmd" ]]; then
        local __cmd="$__agentscribe_last_cmd"
        if [[ "$__cmd" != agentscribe\ * ]]; then
            local __hint
            __hint=$(agentscribe search "$__cmd" -n 1 --solution_only --hint 2>/dev/null)
            if [[ -n "$__hint" ]]; then
                printf '\033[33m[agentscribe] %s\033[0m\n' "$__hint" >&2
            fi
        fi
    fi
    __agentscribe_last_cmd=""
}

autoload -Uz add-zsh-hook
add-zsh-hook preexec __agentscribe_preexec
add-zsh-hook precmd __agentscribe_precmd
"#
        .to_string()
    }
}

fn fish_hook(config: &ShellHookConfig) -> String {
    if config.background {
        r#"# AgentScribe shell hook — auto-query on error (fish)
# Setup: agentscribe shell-hook fish | source  (in ~/.config/fish/config.fish)

function __agentscribe_postexec --on-event fish_postexec
    set -l __exit $status
    set -l __cmd $argv[1]

    # Print hint from previous failed command if the background search finished
    if set -q __agentscribe_hint_file
        set -l __f $__agentscribe_hint_file
        set -e __agentscribe_hint_file
        if test -s "$__f"
            printf '\033[33m[agentscribe] %s\033[0m\n' (string trim (cat "$__f")) >&2
        end
        command rm -f "$__f" 2>/dev/null
    end

    # On non-zero exit, start a background search for the failed command
    if test $__exit -ne 0
        and test -n "$__cmd"
        and not string match -q 'agentscribe *' -- "$__cmd"
        set -l __f (mktemp /tmp/as_hint.XXXXXX 2>/dev/null)
        if test -n "$__f"
            set -g __agentscribe_hint_file "$__f"
            agentscribe search "$__cmd" -n 1 --solution_only --hint > "$__f" 2>/dev/null &
            disown 2>/dev/null; true
        end
    end
end
"#
        .to_string()
    } else {
        r#"# AgentScribe shell hook — auto-query on error (fish, blocking mode)
# Setup: agentscribe shell-hook fish | source  (in ~/.config/fish/config.fish)

function __agentscribe_postexec --on-event fish_postexec
    set -l __exit $status
    set -l __cmd $argv[1]
    if test $__exit -ne 0
        and test -n "$__cmd"
        and not string match -q 'agentscribe *' -- "$__cmd"
        set -l __hint (agentscribe search "$__cmd" -n 1 --solution_only --hint 2>/dev/null)
        if test -n "$__hint"
            printf '\033[33m[agentscribe] %s\033[0m\n' "$__hint" >&2
        end
    end
end
"#
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ShellHookConfig;

    fn default_config() -> ShellHookConfig {
        ShellHookConfig::default()
    }

    #[test]
    fn test_bash_hook_contains_prompt_command() {
        let snippet = generate_hook("bash", &default_config()).unwrap();
        assert!(snippet.contains("PROMPT_COMMAND"));
        assert!(snippet.contains("__agentscribe_prompt_cmd"));
        assert!(snippet.contains("agentscribe search"));
        assert!(snippet.contains("--hint"));
    }

    #[test]
    fn test_zsh_hook_contains_precmd() {
        let snippet = generate_hook("zsh", &default_config()).unwrap();
        assert!(snippet.contains("add-zsh-hook precmd"));
        assert!(snippet.contains("add-zsh-hook preexec"));
        assert!(snippet.contains("__agentscribe_precmd"));
        assert!(snippet.contains("--hint"));
    }

    #[test]
    fn test_fish_hook_contains_postexec() {
        let snippet = generate_hook("fish", &default_config()).unwrap();
        assert!(snippet.contains("fish_postexec"));
        assert!(snippet.contains("--hint"));
    }

    #[test]
    fn test_unsupported_shell_errors() {
        let err = generate_hook("powershell", &default_config()).unwrap_err();
        assert!(err.to_string().contains("unsupported shell"));
    }

    #[test]
    fn test_background_false_omits_tempfile() {
        let config = ShellHookConfig {
            background: false,
            stderr_capture: false,
        };
        let snippet = generate_hook("bash", &config).unwrap();
        assert!(!snippet.contains("mktemp"));
        assert!(snippet.contains("PROMPT_COMMAND"));
    }

    #[test]
    fn test_background_true_uses_tempfile() {
        let snippet = generate_hook("bash", &default_config()).unwrap();
        assert!(snippet.contains("mktemp"));
        assert!(snippet.contains("disown"));
    }

    #[test]
    fn test_snippet_skips_agentscribe_commands() {
        let snippet = generate_hook("bash", &default_config()).unwrap();
        assert!(snippet.contains("agentscribe\\ *"));
    }
}
