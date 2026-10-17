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
        "68d6c8136a08e36b01fc299215d3906bf0dc6c68795e56ee0e546fc3bb34f69a",
    ),
    (
        "kiana-domain/src/memory.rs",
        "7664fd2c0675cf59ff958f75407eb272a30f1bd2c768d1f4594162b140a4d157",
    ),
    (
        "kiana-query/src/index.rs",
        "0515f9a2cce54f3d99edac018f3dc079bfe6d907e0523a365cd42cd44647d56c",
    ),
    (
        "kiana-query/src/repo_map.rs",
        "25e3fe3130ca30ab5e73567c4f0903a24210af1f0a774b1bfb597ff59260fa14",
    ),
    (
        "kiana-core/src/context_query.rs",
        "e4865d44418b195f5fabe8a24d1d9f846f356b87acd2a5bbcdd0215c911282e9",
    ),
    (
        "kiana-daemon/src/context_query.rs",
        "f862a66b00ef8db99ac4c010c65bbc42cbc689944a5e546f87da67fe7025b6f7",
    ),
    (
        "kiana-daemon/src/harness_memory.rs",
        "13e9fdd6bc57a8acecea306f96a39bffffbdaabcb0eaa4e20682db912c92b149",
    ),
    (
        "kiana-daemon/src/memory_retrieval.rs",
        "06dfbbab7f45cc94826279876e3d97a008fb42b00f154855e8bd2b0ff68a36f9",
    ),
    (
        "kiana-core/src/memory_proposals.rs",
        "5f0a041c09235c80c29f097c5afbe6200d00615e8706020a425478cbb648daa7",
    ),
    (
        "kiana-core/src/memory_distillation.rs",
        "1e201441f411e96a978b12338ecdb8459553bd4ced6e7d6e32930d17cf11768d",
    ),
    (
        "kiana-runner/src/compact.rs",
        "6e3c08972ca24ac02334fafada0f689c5302eb6bc0b8ad6291acbb18cef063f6",
    ),
    (
        "kiana-daemon/src/data_governance.rs",
        "fa9a03f688b445c2120589ce51d183d1fcec4266b864f1ddf8cfa5e58fdfca35",
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
    assert_eq!(
        REFERENCE_DIRECTORIES.len(),
        72,
        "CM-00 fixed 72 non-hidden project directories"
    );

    let reference = workspace_root().join("reference");
    if !reference.is_dir() {
        eprintln!(
            "CM-00: reference/ 不在此 checkout 中，跳过目录 inventory 比对；\
             存在 reference/ 时仍按固定清单校验"
        );
        return;
    }
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
}
