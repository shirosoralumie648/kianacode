use crate::registry::create_default_command_registry;
use crate::types::{
    Command, CommandContext, CommandResult, CommandType, COMMAND_ARGV_APP_STATE_KEY,
};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use std::collections::BTreeSet;
use std::path::PathBuf;

pub struct CompletionCommand;

#[async_trait]
impl Command for CompletionCommand {
    fn name(&self) -> &str {
        "completion"
    }

    fn description(&self) -> &str {
        "Generate shell completion scripts"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        let args = command_context_argv(&context).unwrap_or_else(|| split_words(&context.args));
        if args.is_empty() || is_help_arg(args.first().map(String::as_str).unwrap_or_default()) {
            return Ok(CommandResult::text(usage()));
        }

        let parsed = parse_completion_args(&args)?;
        let script = generate_completion_script(parsed.shell)?;
        if let Some(path) = parsed.output {
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            std::fs::write(&path, &script)
                .with_context(|| format!("failed to write {}", path.display()))?;
            return Ok(CommandResult::text(format!(
                "Wrote completion script: {}",
                path.display()
            )));
        }
        Ok(CommandResult::text(script))
    }
}

struct ParsedCompletionArgs {
    shell: Shell,
    output: Option<PathBuf>,
}

#[derive(Clone, Copy)]
enum Shell {
    Bash,
    Zsh,
    Fish,
}

fn parse_completion_args(args: &[String]) -> anyhow::Result<ParsedCompletionArgs> {
    let shell = match args.first().map(String::as_str) {
        Some("bash") => Shell::Bash,
        Some("zsh") => Shell::Zsh,
        Some("fish") => Shell::Fish,
        Some(other) if is_help_arg(other) => return Err(anyhow!(usage())),
        Some(other) => {
            return Err(anyhow!(
                "unsupported completion shell '{}'; expected bash, zsh, or fish\n\n{}",
                other,
                usage()
            ))
        }
        None => return Err(anyhow!(usage())),
    };

    let mut output = None;
    let mut index = 1;
    while index < args.len() {
        let token = args[index].as_str();
        if token == "--output" {
            index += 1;
            let value = args
                .get(index)
                .ok_or_else(|| anyhow!("usage: kiana completion <shell> --output <file>"))?;
            output = Some(PathBuf::from(value));
            index += 1;
            continue;
        }
        if let Some(value) = token.strip_prefix("--output=") {
            output = Some(PathBuf::from(value));
            index += 1;
            continue;
        }
        if is_help_arg(token) {
            return Err(anyhow!(usage()));
        }
        return Err(anyhow!(
            "unknown completion option '{}'\n\n{}",
            token,
            usage()
        ));
    }

    Ok(ParsedCompletionArgs { shell, output })
}

fn generate_completion_script(shell: Shell) -> anyhow::Result<String> {
    let commands = completion_commands();
    let command_words = commands.iter().cloned().collect::<Vec<_>>().join(" ");
    Ok(match shell {
        Shell::Bash => bash_completion(&command_words),
        Shell::Zsh => zsh_completion(&commands),
        Shell::Fish => fish_completion(&commands),
    })
}

fn completion_commands() -> BTreeSet<String> {
    let mut commands = BTreeSet::from([
        "attach".to_string(),
        "agents".to_string(),
        "auth".to_string(),
        "auto-mode".to_string(),
        "bridge".to_string(),
        "chrome".to_string(),
        "chrome-native-host".to_string(),
        "completion".to_string(),
        "daemon".to_string(),
        "deeplink".to_string(),
        "kill".to_string(),
        "list".to_string(),
        "logs".to_string(),
        "mcp".to_string(),
        "mcp-server".to_string(),
        "mcp-server-http".to_string(),
        "mcp-server-sse".to_string(),
        "mcp-server-ws".to_string(),
        "new".to_string(),
        "open".to_string(),
        "ps".to_string(),
        "remote".to_string(),
        "remote-session".to_string(),
        "rename".to_string(),
        "reply".to_string(),
        "show".to_string(),
        "server".to_string(),
        "tui".to_string(),
        "url".to_string(),
    ]);
    for command in create_default_command_registry().list() {
        commands.insert(command.name().to_string());
    }
    commands
}

fn bash_completion(command_words: &str) -> String {
    format!(
        r#"# kiana shell completion for bash
_kiana() {{
  local cur subcommands
  COMPREPLY=()
  cur="${{COMP_WORDS[COMP_CWORD]}}"

  if [[ $COMP_CWORD -eq 1 ]]; then
    COMPREPLY=( $(compgen -W "{command_words}" -- "$cur") )
    return 0
  fi

  case "${{COMP_WORDS[1]}}" in
    auto-mode)
      subcommands="defaults config critique"
      ;;
    auth)
      subcommands="status login logout"
      ;;
    mcp)
      subcommands="status serve list get add add-json add-from-claude-desktop remove reset-project-choices"
      ;;
    plugin)
      subcommands="list status json marketplace install uninstall remove rm show enable disable path validate"
      if [[ "${{COMP_WORDS[COMP_CWORD-1]}}" == "--scope" || "${{COMP_WORDS[COMP_CWORD-1]}}" == "-s" ]]; then
        COMPREPLY=( $(compgen -W "user project local" -- "$cur") )
        return 0
      fi
      if [[ "$cur" == -* ]]; then
        COMPREPLY=( $(compgen -W "--scope -s" -- "$cur") )
        return 0
      fi
      ;;
    remote-session)
      subcommands="status list show rename create archive environments code-session url listen send"
      ;;
    session|list|show|rename)
      subcommands="new list status path current reply show rename tag fork delete export compact"
      ;;
    *)
      subcommands=""
      ;;
  esac

  COMPREPLY=( $(compgen -W "$subcommands" -- "$cur") )
}}
complete -F _kiana kiana
"#
    )
}

fn zsh_completion(commands: &BTreeSet<String>) -> String {
    let command_entries = commands
        .iter()
        .map(|command| format!("    '{}:kiana command'", zsh_escape(command)))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r#"#compdef kiana

_kiana() {{
  local -a commands
  commands=(
{command_entries}
  )

  _arguments -C \
    '1:command:->commands' \
    '*::argument:->arguments'

  case $state in
    commands)
      _describe 'kiana command' commands
      ;;
    arguments)
      case $words[2] in
        auto-mode)
          _values 'auto-mode command' defaults config critique
          ;;
        auth)
          _values 'auth command' status login logout
          ;;
        mcp)
          _values 'mcp command' status serve list get add add-json add-from-claude-desktop remove reset-project-choices
          ;;
        plugin)
          _values 'plugin command' list status json marketplace install uninstall remove rm show enable disable path validate
          _values 'plugin scope' user project local
          ;;
      esac
      ;;
  esac
}}

_kiana "$@"
"#
    )
}

fn fish_completion(commands: &BTreeSet<String>) -> String {
    let mut lines = vec!["# kiana shell completion for fish".to_string()];
    for command in commands {
        lines.push(format!(
            "complete -c kiana -f -a '{}' -d 'kiana command'",
            fish_escape(command)
        ));
    }
    lines.extend([
        "complete -c kiana -f -n '__fish_seen_subcommand_from auto-mode' -a 'defaults config critique'".to_string(),
        "complete -c kiana -f -n '__fish_seen_subcommand_from auth' -a 'status login logout'".to_string(),
        "complete -c kiana -f -n '__fish_seen_subcommand_from mcp' -a 'status serve list get add add-json add-from-claude-desktop remove reset-project-choices'".to_string(),
        "complete -c kiana -f -n '__fish_seen_subcommand_from plugin' -a 'list status json marketplace install uninstall remove rm show enable disable path validate'".to_string(),
        "complete -c kiana -f -n '__fish_seen_subcommand_from plugin' -l scope -s s -d 'plugin scope'".to_string(),
        "complete -c kiana -f -n '__fish_seen_subcommand_from plugin; and __fish_seen_argument -l scope -s s' -a 'user project local'".to_string(),
    ]);
    lines.push(String::new());
    lines.join("\n")
}

fn command_context_argv(context: &CommandContext) -> Option<Vec<String>> {
    context
        .app_state
        .get(COMMAND_ARGV_APP_STATE_KEY)?
        .as_array()?
        .iter()
        .map(|value| value.as_str().map(str::to_string))
        .collect()
}

fn split_words(input: &str) -> Vec<String> {
    input.split_whitespace().map(str::to_string).collect()
}

fn is_help_arg(arg: &str) -> bool {
    matches!(arg, "help" | "--help" | "-h")
}

fn zsh_escape(value: &str) -> String {
    value.replace('\'', "'\\''")
}

fn fish_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}

fn usage() -> &'static str {
    "Usage: kiana completion <shell> [--output <file>]\n       Generate shell completion script for bash, zsh, or fish."
}
