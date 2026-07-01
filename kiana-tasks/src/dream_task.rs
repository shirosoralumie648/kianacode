use crate::task_id::create_task_id_for_type;
use crate::types::{DreamPhase, DreamTaskState, DreamTurn, TaskStateBase, TaskStatus, TaskType};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_TURNS: usize = 30;

pub fn create_dream_task(sessions_reviewing: usize, prior_mtime: u64) -> DreamTaskState {
    let id = create_task_id_for_type("dream");
    let start_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    DreamTaskState {
        base: TaskStateBase {
            id,
            task_type: TaskType::Dream,
            status: TaskStatus::Running,
            description: "dreaming".to_string(),
            tool_use_id: None,
            start_time,
            end_time: None,
            total_paused_ms: None,
            output_file: String::new(),
            output_offset: 0,
            notified: false,
        },
        phase: DreamPhase::Starting,
        sessions_reviewing,
        files_touched: Vec::new(),
        turns: Vec::new(),
        prior_mtime,
    }
}

pub fn add_dream_turn(task: &mut DreamTaskState, turn: DreamTurn, touched_paths: Vec<String>) {
    if turn.text.is_empty() && turn.tool_use_count == 0 && touched_paths.is_empty() {
        return;
    }

    if !touched_paths.is_empty() {
        task.phase = DreamPhase::Updating;
        for path in touched_paths {
            if !task.files_touched.contains(&path) {
                task.files_touched.push(path);
            }
        }
    }

    task.turns.push(turn);
    if task.turns.len() > MAX_TURNS {
        task.turns.drain(0..task.turns.len() - MAX_TURNS);
    }
}

pub fn complete_dream_task(task: &mut DreamTaskState) {
    task.base.status = TaskStatus::Completed;
    task.base.end_time = Some(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64,
    );
    task.base.notified = true;
}

pub fn fail_dream_task(task: &mut DreamTaskState) {
    task.base.status = TaskStatus::Failed;
    task.base.end_time = Some(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64,
    );
    task.base.notified = true;
}

pub fn kill_dream_task(task: &mut DreamTaskState) {
    task.base.status = TaskStatus::Killed;
    task.base.end_time = Some(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64,
    );
    task.base.notified = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_dream_task() {
        let task = create_dream_task(5, 1000);
        assert_eq!(task.base.status, TaskStatus::Running);
        assert_eq!(task.phase, DreamPhase::Starting);
        assert_eq!(task.sessions_reviewing, 5);
        assert!(task.turns.is_empty());
    }

    #[test]
    fn test_add_dream_turn() {
        let mut task = create_dream_task(1, 1000);
        let turn = DreamTurn {
            text: "test".to_string(),
            tool_use_count: 2,
        };
        add_dream_turn(&mut task, turn, vec!["file.txt".to_string()]);

        assert_eq!(task.phase, DreamPhase::Updating);
        assert_eq!(task.turns.len(), 1);
        assert_eq!(task.files_touched.len(), 1);
    }
}
