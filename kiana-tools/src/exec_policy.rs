#[derive(Debug, Clone)]
struct ShellToken {
    text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuoteKind {
    Single,
    Double,
}

pub fn validate_bash_command(command: &str) -> Result<(), String> {
    validate_bash_command_with_depth(command, 0)
}

fn validate_bash_command_with_depth(command: &str, depth: usize) -> Result<(), String> {
    if depth > 8 {
        return Err(deny_reason(
            "nested shell command inspection depth exceeded",
        ));
    }

    let compact = compact_command(command);
    if compact.contains(":(){:|:&};:") {
        return Err(deny_reason("fork bomb pattern"));
    }

    for segment in shell_command_segments(command) {
        let Some(command_index) = command_index(&segment) else {
            continue;
        };
        let command_name = normalized_bash_command_name(&segment[command_index].text);

        if command_name == "sudo" {
            return Err(deny_reason("sudo escalation"));
        }
        if bash_shell_wrapper(&command_name) {
            if let Some(script) = shell_c_argument(&segment, command_index) {
                validate_bash_command_with_depth(script, depth + 1)?;
            }
        }
        if command_name == "mkfs" || command_name.starts_with("mkfs.") {
            return Err(deny_reason("filesystem formatting command"));
        }
        if matches!(
            command_name.as_str(),
            "shutdown" | "reboot" | "poweroff" | "halt"
        ) {
            return Err(deny_reason("host power-management command"));
        }
        if command_name == "rm" && bash_rm_recursive_force_root(&segment[command_index + 1..]) {
            return Err(deny_reason("recursive force removal of filesystem root"));
        }
        if command_name == "dd" && bash_dd_writes_block_device(&segment[command_index + 1..]) {
            return Err(deny_reason("raw block-device write with dd"));
        }
        if command_name == "chmod"
            && bash_chmod_recursive_world_writable_root(&segment[command_index + 1..])
        {
            return Err(deny_reason(
                "recursive world-writable chmod on filesystem root",
            ));
        }
    }

    Ok(())
}

pub fn validate_powershell_command(command: &str) -> Result<(), String> {
    validate_powershell_command_with_depth(command, 0)
}

fn validate_powershell_command_with_depth(command: &str, depth: usize) -> Result<(), String> {
    if depth > 8 {
        return Err(deny_reason(
            "nested PowerShell command inspection depth exceeded",
        ));
    }

    for segment in shell_command_segments(command) {
        let Some(command_index) = command_index(&segment) else {
            continue;
        };
        let command_name = normalized_powershell_command_name(&segment[command_index].text);

        if powershell_wrapper(&command_name) {
            if let Some(script) = powershell_command_argument(&segment, command_index) {
                validate_powershell_command_with_depth(script, depth + 1)?;
            }
        }
        if matches!(
            command_name.as_str(),
            "clear-disk"
                | "initialize-disk"
                | "remove-partition"
                | "format-volume"
                | "restart-computer"
                | "stop-computer"
        ) {
            return Err(deny_reason("destructive system PowerShell command"));
        }
        if matches!(
            command_name.as_str(),
            "remove-item" | "rm" | "del" | "erase" | "rd" | "rmdir"
        ) && powershell_remove_item_recursive_force_root(&segment[command_index + 1..])
        {
            return Err(deny_reason(
                "recursive force removal of a filesystem root or drive root",
            ));
        }
    }

    Ok(())
}

fn deny_reason(reason: &str) -> String {
    format!("exec policy denied command: {reason}")
}

fn shell_command_segments(command: &str) -> Vec<Vec<ShellToken>> {
    let mut segments = Vec::new();
    let mut segment = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut started = false;

    for ch in command.chars() {
        if escaped {
            current.push(ch);
            started = true;
            escaped = false;
            continue;
        }

        match quote {
            Some(QuoteKind::Single) => {
                if ch == '\'' {
                    quote = None;
                } else {
                    current.push(ch);
                }
                started = true;
            }
            Some(QuoteKind::Double) => match ch {
                '"' => {
                    quote = None;
                    started = true;
                }
                '\\' => {
                    escaped = true;
                    started = true;
                }
                '`' => {
                    push_token(&mut segment, &mut current, &mut started);
                    push_segment(&mut segments, &mut segment);
                }
                other => {
                    current.push(other);
                    started = true;
                }
            },
            None => match ch {
                '\'' => {
                    quote = Some(QuoteKind::Single);
                    started = true;
                }
                '"' => {
                    quote = Some(QuoteKind::Double);
                    started = true;
                }
                '\\' => {
                    escaped = true;
                    started = true;
                }
                '`' => {
                    push_token(&mut segment, &mut current, &mut started);
                    push_segment(&mut segments, &mut segment);
                }
                ch if ch.is_whitespace() => {
                    push_token(&mut segment, &mut current, &mut started);
                }
                ';' | '&' | '|' | '(' | ')' => {
                    push_token(&mut segment, &mut current, &mut started);
                    push_segment(&mut segments, &mut segment);
                }
                other => {
                    current.push(other);
                    started = true;
                }
            },
        }
    }

    if escaped {
        current.push('\\');
        started = true;
    }
    push_token(&mut segment, &mut current, &mut started);
    push_segment(&mut segments, &mut segment);
    segments
}

fn push_token(segment: &mut Vec<ShellToken>, current: &mut String, started: &mut bool) {
    if !*started {
        return;
    }
    let text = clean_token(current);
    if !text.is_empty() {
        segment.push(ShellToken { text });
    }
    current.clear();
    *started = false;
}

fn push_segment(segments: &mut Vec<Vec<ShellToken>>, segment: &mut Vec<ShellToken>) {
    if !segment.is_empty() {
        segments.push(std::mem::take(segment));
    }
}

fn clean_token(token: &str) -> String {
    token
        .trim_matches(|ch: char| {
            matches!(
                ch,
                '"' | '\'' | ',' | '[' | ']' | '{' | '}' | '\r' | '\n' | '\t'
            )
        })
        .to_ascii_lowercase()
}

fn compact_command(command: &str) -> String {
    command
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase()
}

fn command_index(segment: &[ShellToken]) -> Option<usize> {
    let mut index = 0;
    let mut env_wrapper = false;

    while index < segment.len() {
        let word = &segment[index].text;
        if !env_wrapper && assignment_word(word) {
            index += 1;
            continue;
        }
        if !env_wrapper && normalized_bash_command_name(word) == "env" {
            env_wrapper = true;
            index += 1;
            continue;
        }
        if env_wrapper && (assignment_word(word) || word.starts_with('-')) {
            index += 1;
            continue;
        }
        return Some(index);
    }

    None
}

fn assignment_word(word: &str) -> bool {
    let Some((name, _value)) = word.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && !name.starts_with('-')
        && name
            .chars()
            .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
        && !name.chars().next().is_some_and(|ch| ch.is_ascii_digit())
}

fn normalized_bash_command_name(command: &str) -> String {
    command_basename(command).to_string()
}

fn normalized_powershell_command_name(command: &str) -> String {
    let mut command = command_basename(command).to_string();
    for suffix in [".exe", ".cmd", ".bat", ".ps1", ".psm1"] {
        if let Some(stripped) = command.strip_suffix(suffix) {
            command = stripped.to_string();
            break;
        }
    }
    command
}

fn command_basename(command: &str) -> &str {
    command
        .rsplit(|ch| matches!(ch, '/' | '\\'))
        .next()
        .unwrap_or(command)
}

fn bash_shell_wrapper(command: &str) -> bool {
    matches!(
        command,
        "bash" | "sh" | "zsh" | "dash" | "ash" | "ksh" | "mksh"
    )
}

fn powershell_wrapper(command: &str) -> bool {
    matches!(command, "pwsh" | "powershell")
}

fn shell_c_argument(segment: &[ShellToken], command_index: usize) -> Option<&str> {
    let args = &segment[command_index + 1..];
    for (index, token) in args.iter().enumerate() {
        let word = token.text.as_str();
        if word == "-c" || (word.starts_with('-') && word[1..].contains('c')) {
            return args.get(index + 1).map(|token| token.text.as_str());
        }
    }
    None
}

fn powershell_command_argument(segment: &[ShellToken], command_index: usize) -> Option<&str> {
    let args = &segment[command_index + 1..];
    for (index, token) in args.iter().enumerate() {
        if matches!(
            token.text.as_str(),
            "-command" | "-c" | "/command" | "/c" | "-encodedcommand" | "-enc"
        ) {
            return args.get(index + 1).map(|token| token.text.as_str());
        }
    }
    None
}

fn bash_rm_recursive_force_root(args: &[ShellToken]) -> bool {
    let recursive_force = args.iter().any(|token| {
        token.text.starts_with('-') && token.text.contains('r') && token.text.contains('f')
    });
    recursive_force && args.iter().any(|token| bash_root_target(&token.text))
}

fn bash_dd_writes_block_device(args: &[ShellToken]) -> bool {
    args.iter().any(|token| {
        token
            .text
            .strip_prefix("of=")
            .is_some_and(block_device_target)
    })
}

fn bash_chmod_recursive_world_writable_root(args: &[ShellToken]) -> bool {
    let recursive = args
        .iter()
        .any(|token| token.text == "-r" || token.text.eq_ignore_ascii_case("-r"));
    let world_writable = args
        .iter()
        .any(|token| token.text == "777" || token.text == "0777");
    recursive && world_writable && args.iter().any(|token| bash_root_target(&token.text))
}

fn powershell_remove_item_recursive_force_root(args: &[ShellToken]) -> bool {
    let recursive = args
        .iter()
        .any(|token| matches!(token.text.as_str(), "-recurse" | "-r"));
    let force = args.iter().any(|token| token.text == "-force");
    recursive && force && args.iter().any(|token| powershell_root_target(&token.text))
}

fn bash_root_target(token: &str) -> bool {
    matches!(token, "/" | "/*" | "/." | "/.." | "\"/\"" | "'/'")
}

fn powershell_root_target(token: &str) -> bool {
    matches!(token, "/" | "\\" | "/*" | "\\*" | "c:" | "c:\\" | "c:/")
        || token.ends_with(":\\")
        || token.ends_with(":/")
}

fn block_device_target(target: &str) -> bool {
    target == "/dev" || target.starts_with("/dev/")
}
