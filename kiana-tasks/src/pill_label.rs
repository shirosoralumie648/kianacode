use crate::types::{BashTaskKind, TaskState};

const DIAMOND_OPEN: &str = "◇";

pub fn get_pill_label(tasks: &[TaskState]) -> String {
    let n = tasks.len();
    if n == 0 {
        return String::new();
    }

    let all_same_type = tasks
        .iter()
        .all(|t| std::mem::discriminant(t) == std::mem::discriminant(&tasks[0]));

    if all_same_type {
        match &tasks[0] {
            TaskState::LocalBash(_) => {
                let monitors = tasks
                    .iter()
                    .filter(|t| {
                        if let TaskState::LocalBash(s) = t {
                            matches!(s.kind, Some(BashTaskKind::Monitor))
                        } else {
                            false
                        }
                    })
                    .count();
                let shells = n - monitors;
                let mut parts = Vec::new();
                if shells > 0 {
                    parts.push(if shells == 1 {
                        "1 shell".to_string()
                    } else {
                        format!("{} shells", shells)
                    });
                }
                if monitors > 0 {
                    parts.push(if monitors == 1 {
                        "1 monitor".to_string()
                    } else {
                        format!("{} monitors", monitors)
                    });
                }
                parts.join(", ")
            }
            TaskState::LocalAgent(_) => {
                if n == 1 {
                    "1 local agent".to_string()
                } else {
                    format!("{} local agents", n)
                }
            }
            TaskState::RemoteAgent(_) => {
                if n == 1 {
                    format!("{} 1 cloud session", DIAMOND_OPEN)
                } else {
                    format!("{} {} cloud sessions", DIAMOND_OPEN, n)
                }
            }
            TaskState::InProcessTeammate(_) => {
                if n == 1 {
                    "1 team".to_string()
                } else {
                    format!("{} teams", n)
                }
            }
            TaskState::LocalWorkflow(_) => {
                if n == 1 {
                    "1 background workflow".to_string()
                } else {
                    format!("{} background workflows", n)
                }
            }
            TaskState::MonitorMcp(_) => {
                if n == 1 {
                    "1 monitor".to_string()
                } else {
                    format!("{} monitors", n)
                }
            }
            TaskState::Dream(_) => "dreaming".to_string(),
        }
    } else {
        format!("{} background {}", n, if n == 1 { "task" } else { "tasks" })
    }
}

pub fn pill_needs_cta(tasks: &[TaskState]) -> bool {
    if tasks.len() != 1 {
        return false;
    }
    matches!(tasks[0], TaskState::RemoteAgent(_))
}
