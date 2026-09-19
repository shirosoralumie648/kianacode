//! Task-lens adapter over the existing read-only RepoMap scanner.

use crate::repo_map::RepoMap;
use kiana_domain::{
    json_digest, memory_tokens, RepoMapSymbol, RepoMapTaskCandidate, RepoMapTaskSelection,
};
use serde_json::json;

pub fn select_repo_map_for_task(
    map: &RepoMap,
    task: impl Into<String>,
    token_budget: u64,
) -> Result<RepoMapTaskSelection, String> {
    let task = task.into();
    let terms = memory_tokens(&task);
    let candidates = map
        .files
        .iter()
        .map(|file| {
            let haystack = format!("{} {}", file.path, file.symbols.join(" ")).to_ascii_lowercase();
            let task_score = terms
                .iter()
                .enumerate()
                .map(|(index, term)| {
                    let matches = haystack.matches(term).count() as u64;
                    matches.saturating_mul((terms.len() - index) as u64)
                })
                .sum();
            let symbols = file
                .symbols
                .iter()
                .map(|symbol| RepoMapSymbol::heuristic(symbol.clone()))
                .collect::<Result<Vec<_>, _>>()?;
            let content_digest = if file.content_hash.starts_with("sha256:") {
                file.content_hash.clone()
            } else {
                json_digest(&json!({"path":file.path,"symbols":file.symbols}))
            };
            Ok(RepoMapTaskCandidate {
                path: file.path.clone(),
                language: file.language.clone(),
                content_digest,
                symbols,
                dependencies: Vec::new(),
                task_score,
                estimated_tokens: file.estimated_tokens.max(1),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    RepoMapTaskSelection::new(task, token_budget, candidates)
}
