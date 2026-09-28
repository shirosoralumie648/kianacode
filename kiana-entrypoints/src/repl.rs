//! 交互式 REPL：终端里的一问一答，加上一组以 `/` 开头的斜杠命令。
//!
//! # 这个文件在系统里的位置
//!
//! ```text
//! 用户在终端敲一行
//!    ↓  【本文件】classify_repl_input —— 分成五类
//!    ├─ /命令        → crate::command_dispatch::dispatch_command → DaemonHost → ControlPlane
//!    ├─ 普通提问      → crate::sdk::unstable_v2_prompt
//!    ├─ !shell       → 改写成一句「请执行这条 shell 命令」的提问，走上面那条
//!    ├─ 未知命令      → 提示并继续
//!    └─ 空行          → 继续
//! ```
//!
//! **上游**：终端。**下游**：两条不同的路，见上。
//!
//! # 两条路的区别值得先知道
//!
//! `/命令` 走的是 `command_dispatch`，也就是本仓库认定的那条唯一授权通道：
//! 界面把命令交给 `DaemonHost`，由 `ControlPlane` 判定后才可能产生副作用。
//!
//! 而普通提问（以及 `!` 快捷）走的是 `sdk::unstable_v2_prompt`。它表面上把
//! permission handler 传成 `None`（`sdk.rs`），看起来像是「不询问就执行」，
//! 但**追下去会发现它并不是一条独立的执行路径**：
//!
//! ```text
//! unstable_v2_prompt
//!    → prompt_with_persistence_at        （sdk.rs）
//!    → execute_owned_harness_turn        （sdk.rs:1078）
//!    → crate::harness_run::HarnessRunResult
//!    → KianaClient → DaemonHost → ControlPlane
//! ```
//!
//! 也就是说提问最终仍然落在本文件上面那条产品执行面上，授权照常发生。
//! `permission_handler` 为 `None` 的后果只是：遇到需要批准的动作时，**不弹窗**，
//! 而是把 `AwaitingApproval` 原样返回给调用方（见 [ `run_repl_prompt` ]）。
//! 决定权仍在控制面，缺的只是呈现。
//!
//! 另外 `sdk::should_execute_model` 默认是 false：没有 `execute` / `run_model` 选项、
//! 也没有 `KIANA_SDK_EXECUTE_MODEL` 时，SDK 只**记录**这次提问而不执行。
//! REPL 在 [ `run_repl_prompt` ] 里显式打开了 `execute`，所以它确实会跑。
//!
//! 这条路与 `mcp.rs` 的差别正在这里：那边不经过控制面，是真的没有授权；
//! 这边只是把「谁来点这个批准」交回给了调用方。
//!
//! 对比同目录下的 `mcp.rs`：那条路的工具执行完全不经过 ControlPlane，
//! 已经作为安全发现记录在
//! [安全发现文档](../../docs/security/mcp-http-unauthenticated-tool-execution.md)。
use anyhow::Result;
use colored::*;
use kiana_commands::{
    create_default_command_registry, CommandContext, CommandRegistry, CommandType,
};
use rustyline::DefaultEditor;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

/// REPL 主循环。
///
/// 【为什么它值得单独一个函数】
/// 因为它同时持有三样有状态的东西：命令注册表、行编辑器的历史、以及跨轮次保留的
/// `app_state` 与 `session_id`。把它们拆出去就没法保存历史了。
///
/// 【`readline` 出错就 break】
/// `Err(_)` 同时覆盖 Ctrl+C 与 Ctrl+D。对一个交互式会话来说，这两者都意味着
/// 「用户要走了」，所以没有必要区分；而 `Ok("")`（空行）不是退出，它会继续下一轮。
///
/// 【分类在每轮的开头做】
/// `classify_repl_input` 先跑，命令走命令、提问走提问。这不是为了省一次函数调用，
/// 而是为了让「这一行到底是命令还是对话」在**任何副作用发生之前**就有答案。
pub async fn run_repl() -> Result<()> {
    let command_registry = create_default_command_registry();
    let mut editor = DefaultEditor::new()?;
    let cwd = std::env::current_dir()?;
    let mut app_state = HashMap::new();
    let mut session_id = create_repl_session(&cwd).await?;

    println!("{}", "━".repeat(60).bright_black());
    println!("{}", "Kiana REPL".bright_cyan().bold());
    println!("{} Session: {}", "ℹ".bright_blue(), session_id);
    println!("{} Press Ctrl+C to exit", "ℹ".bright_blue());
    println!("{}", "━".repeat(60).bright_black());
    println!();

    loop {
        let mut input = match editor.readline(&format!("{} ", "You:".bright_green().bold())) {
            Ok(line) => line,
            Err(_) => break,
        };

        match classify_repl_input(&input, &command_registry) {
            ReplInputRoute::Empty => continue,
            ReplInputRoute::UnknownCommand(name) => {
                editor.add_history_entry(&input)?;
                println!(
                    "{} Unknown command: /{}. Try /help.",
                    "Kiana:".bright_cyan().bold(),
                    name
                );
                println!();
                continue;
            }
            ReplInputRoute::Command { name, args } => {
                editor.add_history_entry(&input)?;
                let command = command_registry
                    .get(&name)
                    .expect("classified command must exist in registry");
                let command_state = command_app_state(&app_state, &session_id, &cwd);
                let should_clear_session = is_clear_session_command(&name, &args);
                let command_context = CommandContext {
                    args,
                    app_state: command_state,
                };
                let result = match crate::command_dispatch::dispatch_command(
                    command.as_ref(),
                    command_context.clone(),
                )
                .await?
                {
                    crate::command_dispatch::CommandDispatchOutcome::Completed(result) => result,
                    crate::command_dispatch::CommandDispatchOutcome::AwaitingApproval(
                        challenge,
                    ) => {
                        println!(
                            "{} Local write approval requested: {}",
                            "Kiana:".bright_cyan().bold(),
                            challenge.reason
                        );
                        let approved = editor
                            .readline("Approve this exact request? [y/N] ")
                            .map(|answer| matches!(answer.trim(), "y" | "Y" | "yes" | "YES"))
                            .unwrap_or(false);
                        if !approved {
                            let _ =
                                crate::command_dispatch::resolve_command_approval_with_challenge(
                                    &command_context,
                                    &challenge,
                                    kiana_protocol::ApprovalDecision::Deny,
                                )
                                .await;
                            println!("{} Local write denied.", "Kiana:".bright_cyan().bold());
                            println!();
                            continue;
                        }
                        crate::command_dispatch::resolve_command_approval_with_challenge(
                            &command_context,
                            &challenge,
                            kiana_protocol::ApprovalDecision::Approve,
                        )
                        .await?
                    }
                };

                if result.output_type == "exit" {
                    if !result.value.is_empty() {
                        println!("{} {}", "Kiana:".bright_cyan().bold(), result.value);
                    }
                    break;
                }

                match command.command_type() {
                    CommandType::Prompt => {
                        input = result.value;
                    }
                    CommandType::Local | CommandType::LocalJsx => {
                        if should_clear_session {
                            session_id = create_repl_session(&cwd).await?;
                            app_state.clear();
                            println!(
                                "{} New session: {}",
                                "Kiana:".bright_cyan().bold(),
                                session_id
                            );
                        } else if !result.value.is_empty() {
                            println!("{} {}", "Kiana:".bright_cyan().bold(), result.value);
                        }
                        println!();
                        continue;
                    }
                }
            }
            ReplInputRoute::Prompt(prompt) => {
                editor.add_history_entry(&input)?;
                input = prompt;
            }
            ReplInputRoute::BashShortcut(command) => {
                editor.add_history_entry(&input)?;
                input = format!("Run this shell command: {command}");
            }
        }

        println!("{} Running...", "Kiana:".bright_cyan().bold());
        match run_repl_prompt(&session_id, &cwd, input).await {
            Ok(text) if text.trim().is_empty() => {
                println!(
                    "{} Completed with no assistant text.",
                    "Kiana:".bright_cyan().bold()
                );
            }
            Ok(text) => {
                println!("{} {}", "Assistant:".bright_cyan().bold(), text);
            }
            Err(error) => {
                eprintln!("{} {}", "✗".red().bold(), format!("Error: {}", error).red());
            }
        }
        println!();
    }

    Ok(())
}

/// 开一个新会话，并返回它的 id。
///
/// 【为什么 id 要落盘而不是随机生成】
/// 因为 `session_id` 是后面每一次提问的关联键：命令执行、审批引用、事件记录都靠它串起来。
/// 一个随手生成的、彼此无关的 id，会让「这轮对话和上一轮到底是不是同一件事」变得无法回答。
///
/// 【工作目录为什么要一起带】
/// 同样是为了让后续每一次请求都知道「相对路径是相对谁」。
async fn create_repl_session(cwd: &Path) -> Result<String> {
    let mut options = HashMap::new();
    options.insert(
        "title".to_string(),
        Value::String(repl_session_title(cwd).to_string()),
    );
    options.insert("tag".to_string(), Value::String("repl".to_string()));
    let session = crate::sdk::unstable_v2_create_session(options).await?;
    Ok(session.session_id)
}

/// 把一句提问送出去，拿回助手的文字。
///
/// 【`execute: true` 是什么】
/// 它告诉 SDK 允许模型在这一轮调用工具。对比「只问不答」的用法，这里是明确放开的。
///
/// ⚠ 注意这条路的 permission handler 是 `None`（`unstable_v2_prompt` 里传的），
/// 也就是**不会询问**。模型是否真的能执行，取决于 `sdk` 那一侧是否另有管控——
/// 这一点本轮没有追到底，不在这里下结论。
///
/// 【为什么只取文字】
/// REPL 是给人看的，只需要模型的回答文本。工具调用、事件、receipt 这些事实留在
/// 它们该在的地方（EventLog 与投影），不进终端回显。
async fn run_repl_prompt(session_id: &str, cwd: &Path, input: String) -> Result<String> {
    let mut options = repl_prompt_options(session_id, cwd);
    options.insert("execute".to_string(), Value::Bool(true));
    let result = crate::sdk::unstable_v2_prompt(input, options).await?;
    Ok(assistant_text_from_result(&result))
}

fn repl_prompt_options(session_id: &str, cwd: &Path) -> HashMap<String, Value> {
    HashMap::from([
        (
            "session_id".to_string(),
            Value::String(session_id.to_string()),
        ),
        (
            "cwd".to_string(),
            Value::String(cwd.to_string_lossy().to_string()),
        ),
    ])
}

fn repl_session_title(cwd: &Path) -> String {
    let name = cwd
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(".");
    format!("REPL {}", name)
}

fn assistant_text_from_result(result: &Value) -> String {
    result
        .get("assistant_text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn command_app_state(
    app_state: &HashMap<String, Value>,
    session_id: &str,
    cwd: &Path,
) -> HashMap<String, Value> {
    let mut state = app_state.clone();
    state.insert(
        "session_id".to_string(),
        Value::String(session_id.to_string()),
    );
    state.insert(
        "cwd".to_string(),
        Value::String(cwd.to_string_lossy().to_string()),
    );
    state
}

/// 判断这一条是不是「清空会话」。
///
/// 【为什么要在执行前单独判断】
/// 因为这条命令会**改变后面所有请求的归属**。如果先执行、后判断，那这一轮已经带着
/// 旧的 session 发出去了，新旧会话的边界就会错一位。
///
/// 先算出来、最后再决定要不要清，边界才是干净的。
fn is_clear_session_command(name: &str, args: &str) -> bool {
    name == "clear" && args.trim().is_empty()
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// 一行输入的分类结果。
///
/// 【为什么用枚举而不是边解析边执行】
/// 因为「这一行是什么」必须先确定，才能决定它该走哪条路。边解析边执行意味着
/// 解析到一半就可能已经产生了副作用，而那时你还不知道自己在处理命令还是在对话。
///
/// 五个变体里值得注意的是 `BashShortcut`：它**不代表执行 shell**，
/// 名字有点误导，注释在 `classify_repl_input` 里说明。
enum ReplInputRoute {
    Empty,
    Prompt(String),
    Command { name: String, args: String },
    UnknownCommand(String),
    BashShortcut(String),
}

/// 把一行输入分成五类之一。**这个函数不产生任何副作用。**
///
/// 【判定顺序有讲究】
/// 1. 先去掉首尾空白再看空不空——空行是「用户还在想」，不是「用户想执行什么」；
/// 2. 再看 `!` 开头。把它排在 `/` 前面是因为两者首字符不同，顺序其实无所谓，
///    但**必须都排在「是命令吗」这个判断之前**；
/// 3. 只有既不是 `!` 也不是 `/` 开头，才落到 `Prompt`。
///
/// 【`/name` 是不是命令，由注册表说了算】
/// 最后一步是 `command_registry.get(&name).is_some()`。也就是说：斜杠前缀本身
/// **不构成**一个命令，只有注册表里确实有这个名字才算。
///
/// 这一点很重要，否则任何以 `/` 开头的输入都会被当成命令送下去，而
/// 「用户只是想打一个以斜杠开头的句子」这种情况就会变成一次莫名的命令错误。
///
/// 【⚠ `!` 快捷不执行 shell】
/// 看到 `!ls -la` 时它返回 `BashShortcut("ls -la")`，而调用方**不会**去跑 shell。
/// 它把输入改写成一句自然语言（`Run this shell command: ls -la`），再当作普通提问发给模型。
/// 所以 `!` 是「少打几个字的快捷方式」，不是「绕过授权的直通车」——
/// 真要执行，仍然要经过模型与它背后的那条链路。
///
/// 【参数只切一刀】
/// `splitn(2, char::is_whitespace)`：第一个空白之后**全部**都是参数，
/// 内部的空格原样保留。因为参数里出现空格是常态（路径、带空格的查询），
/// 而命令名里出现空格不是。
fn classify_repl_input(input: &str, command_registry: &CommandRegistry) -> ReplInputRoute {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return ReplInputRoute::Empty;
    }

    if let Some(command) = trimmed.strip_prefix('!') {
        let command = command.trim();
        if !command.is_empty() {
            return ReplInputRoute::BashShortcut(command.to_string());
        }
    }

    let Some(command_line) = trimmed.strip_prefix('/') else {
        return ReplInputRoute::Prompt(input.to_string());
    };

    let mut parts = command_line.splitn(2, char::is_whitespace);
    let name = parts.next().unwrap_or_default().to_string();
    let args = parts.next().unwrap_or_default().trim().to_string();
    if command_registry.get(&name).is_some() {
        ReplInputRoute::Command { name, args }
    } else {
        ReplInputRoute::UnknownCommand(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn repl_prompt_options_include_session_and_cwd() {
        let options = repl_prompt_options("session-1", Path::new("/tmp/work"));

        assert_eq!(options["session_id"], json!("session-1"));
        assert_eq!(options["cwd"], json!("/tmp/work"));
        assert!(options.get("execute").is_none());
    }

    #[test]
    fn repl_session_title_uses_cwd_name() {
        assert_eq!(
            repl_session_title(Path::new("/tmp/kianacode")),
            "REPL kianacode"
        );
        assert_eq!(repl_session_title(Path::new("/")), "REPL .");
    }

    #[test]
    fn assistant_text_defaults_to_empty_string() {
        assert_eq!(
            assistant_text_from_result(&json!({"assistant_text": "done"})),
            "done"
        );
        assert_eq!(assistant_text_from_result(&json!({})), "");
    }

    #[test]
    fn command_app_state_includes_current_session_context() {
        let state = command_app_state(&HashMap::new(), "session-1", Path::new("/tmp/work"));

        assert_eq!(state["session_id"], json!("session-1"));
        assert_eq!(state["cwd"], json!("/tmp/work"));
    }

    #[test]
    fn clear_session_command_requires_no_args() {
        assert!(is_clear_session_command("clear", ""));
        assert!(is_clear_session_command("clear", "   "));
        assert!(!is_clear_session_command("clear", "status"));
        assert!(!is_clear_session_command("clear", "--help"));
        assert!(!is_clear_session_command("status", ""));
    }

    #[test]
    fn repl_input_classifier_routes_text_slash_bash_and_unknown_commands() {
        let registry = create_default_command_registry();

        assert_eq!(classify_repl_input("   ", &registry), ReplInputRoute::Empty);
        assert_eq!(
            classify_repl_input("explain this file", &registry),
            ReplInputRoute::Prompt("explain this file".to_string())
        );
        assert_eq!(
            classify_repl_input("please run /help", &registry),
            ReplInputRoute::Prompt("please run /help".to_string())
        );
        assert_eq!(
            classify_repl_input("/help config", &registry),
            ReplInputRoute::Command {
                name: "help".to_string(),
                args: "config".to_string()
            }
        );
        assert_eq!(
            classify_repl_input("/unknown arg", &registry),
            ReplInputRoute::UnknownCommand("unknown".to_string())
        );
        assert_eq!(
            classify_repl_input("!git status --short", &registry),
            ReplInputRoute::BashShortcut("git status --short".to_string())
        );
    }
}
