//! `CM-00` Context / Memory 基线复现测试。
//!
//! 本文件固定 source-only 基线：关键入口文件的 SHA-256 与 `reference/` 目录
//! inventory。任何漂移（有意修改或无意损坏）都会让测试变红，由修改者同步
//! `docs/roadmap/context-memory-baseline.md` 的快照表后更新这里的期望值。
//! 这些测试不运行产品代码，不构成 local_behavior 证明；行为验收由后续
//! `CM-*` 卡的测试和 GitHub CI 负责。

use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn sha256_of(path: &Path) -> String {
    let bytes = fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    format!("{:x}", Sha256::digest(&bytes))
}

/// `CM-00` 基线快照：context / memory 主链入口文件的 SHA-256。
/// 与 `docs/roadmap/context-memory-baseline.md` §1 的快照表一一对应。
const BASELINE: &[(&str, &str)] = &[
    (
        "kiana-domain/src/prompts.rs",
        "ddcf19d5f449d11fac615a0e1ebff4c25900da183e359e9285a38e4f48bb69bd",
    ),
    (
        "kiana-domain/src/memory.rs",
        "7e7330b09e582af7871b0bf6cdfb11a9c62001326bb2f21fb829cff05dadde2f",
    ),
    (
        "kiana-query/src/index.rs",
        "a9cfc4767f24a7f790686be8068554aac7ee84c6c8d58ee6c71e78b75afc3716",
    ),
    (
        "kiana-query/src/repo_map.rs",
        "ead92a58e5de2b391779666007b4972bc79ef0e16226e8937338310ae9e39100",
    ),
    (
        "kiana-core/src/context_query.rs",
        "c7694ad6ced9e4b7269c7e9961efb4216861b35c32a5bdaf016e931e281b02ae",
    ),
    (
        "kiana-daemon/src/context_query.rs",
        "4a85812ed61f04e81eb2a16bd1c01044b08540c6457aba289282975c97ff283b",
    ),
    (
        "kiana-daemon/src/harness_memory.rs",
        "6379bfb054a745f07f9977cf63122107f97e3753b5e15bcfc90217d8518b953c",
    ),
    (
        "kiana-daemon/src/memory_retrieval.rs",
        "c41a0a9af561904df76ba0391dd010b384e2ff9dcaaeb95e5e2b4d3be5a3dd5b",
    ),
    (
        "kiana-core/src/memory_proposals.rs",
        "f59cc310bc5e16960e967da4e1dd62d4fc63cfb6ef7ee69d39d3faca2578e876",
    ),
    (
        "kiana-core/src/memory_distillation.rs",
        "a06724647ee589a998c47c11f97e3c646206d4a57fa32dcfc1c108974633b0b8",
    ),
    (
        "kiana-runner/src/compact.rs",
        "bd021351b20331da36762ffcb55348856d5c1429833c3e703593ab5017325a4d",
    ),
    (
        "kiana-daemon/src/data_governance.rs",
        "bbecc5f6c1e11d9e09de953ea2313f1c4976ee6aa07dc4833625b23931e2ccf3",
    ),
];

#[test]
fn context_memory_baseline_is_reproducible() {
    let root = workspace_root();
    for (relative, expected) in BASELINE {
        let actual = sha256_of(&root.join(relative));
        assert_eq!(
            actual, *expected,
            "{relative} drifted from the CM-00 baseline snapshot; if the change is intentional, \
             update docs/roadmap/context-memory-baseline.md and this table together"
        );
    }
}

/// `CM-00` reference inventory：`reference/` 下的非隐藏项目目录集合。
/// 新增或移除参考项目是有意的调研决策，应同步 `docs/roadmap/context-memory.md`
/// §24.3 的 inventory 表后再更新此列表。顶层的数据库/运行资料文件
/// （`agentdb.rvf`、`agentdb.rvf.lock`、`ruvector.db`、`COMPANYOS-REFERENCES.md`）
/// 只登记不计入项目目录；隐藏 `.claude-flow` 同样不计入。
const REFERENCE_DIRECTORIES: &[&str] = &[
    "12-factor-agents",
    "a2a",
    "adk-python",
    "agency-swarm",
    "agent-framework",
    "agno",
    "ai-coding-guide",
    "aider",
    "architect-loop",
    "Archon",
    "Archon-Knowledge",
    "autogen",
    "awesome-agent-skills",
    "beads",
    "ChatDev",
    "claude-code-main (2)",
    "claude-code-rev-main",
    "claude-code-rust",
    "claude-mem-candidate",
    "claude-memory",
    "claude-task-master",
    "cline",
    "codex",
    "container-use",
    "continue",
    "crewAI",
    "crush",
    "deepseek-harness",
    "ECC",
    "emdash",
    "everything-claude-code",
    "gastown",
    "get-shit-done",
    "GitNexus",
    "goose",
    "gpt-pilot",
    "graphify",
    "graphiti",
    "grok-build",
    "gsd-core",
    "gstack",
    "herdr",
    "langchain",
    "langgraph",
    "letta",
    "letta-code",
    "letta-oss",
    "llama-index",
    "mcp-servers",
    "mem0",
    "memorix",
    "MemPalace",
    "MetaGPT",
    "mini-swe-agent",
    "openai-agents-python",
    "opencode",
    "OpenHands",
    "OpenSpec",
    "orca",
    "pi",
    "planning-with-files",
    "pm-skills",
    "promptfoo-full",
    "pydantic-ai",
    "roo-code",
    "Roo-Code",
    "ruflo",
    "skills",
    "spec-kit",
    "strix",
    "superpowers",
    "temporal-sdk-python",
];

#[test]
fn reference_inventory_covers_all_directories() {
    let reference = workspace_root().join("reference");
    let mut on_disk: BTreeSet<String> = fs::read_dir(&reference)
        .unwrap_or_else(|error| panic!("read {}: {error}", reference.display()))
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| !name.starts_with('.'))
        .collect();

    let expected: BTreeSet<String> = REFERENCE_DIRECTORIES
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    let missing: Vec<String> = expected.difference(&on_disk).cloned().collect();
    let unlisted: Vec<String> = on_disk.difference(&expected).cloned().collect();

    assert!(
        missing.is_empty() && unlisted.is_empty(),
        "reference/ inventory drifted from CM-00: missing from disk {missing:?}; on disk but unlisted {unlisted:?}; \
         sync docs/roadmap/context-memory.md §24.3 and this list together"
    );
    assert_eq!(
        REFERENCE_DIRECTORIES.len(),
        72,
        "CM-00 fixed 72 non-hidden project directories"
    );
}
