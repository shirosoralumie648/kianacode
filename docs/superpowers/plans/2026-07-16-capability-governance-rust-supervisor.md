# Capability Governance Rust Supervisor Implementation Plan

> **Execution rule:** This project does not use TDD. Implement each approved supervisor contract first, then add and run focused, adversarial, integration, and review verification. No task requires a pre-implementation failure run.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the fail-open Shell runtime authority in Phase 1 Plan 01-03 with one fail-closed Rust supervisor that alone may publish `offline=true` and public exit `0`.

**Architecture:** Keep the existing Bash/Python schema and fixture semantics behind a hidden worker protocol. A new `std + libc` Rust binary owns trusted launcher selection, sandbox construction, deadline, bounded pipes, trace and receipt parsing, process-tree cleanup, and the final positive-conjunction verdict. The Linux/WSL MVP uses fixed root-owned `bwrap`, `strace`, Bash, and `/bin/sh`; test-only launcher injection stays inside Rust tests and is never selected through production argv or environment.

**Tech Stack:** Rust 1.96, Cargo locked/offline workspace, `std`, `libc`, Bash 5, Bubblewrap, strace, Python isolated mode with `jsonschema` Draft 2020-12, Linux process groups and namespaces.

---

## Execution Boundaries

- Execute on the existing `master` checkout. Do not create or switch a branch or worktree.
- Do not run `$gsd-execute-phase 1` during remediation. `01-04` remains blocked until Task 10 restores `01-03-SUMMARY.md` to `complete`.
- Never use `git add .`, `git add -A`, `git reset`, `git checkout`, or `git add -- Cargo.lock`.
- Treat `.planning/config.json`, `scripts/schema-contract-smoke.sh`, `scripts/validate-json-schema.py`, and the four files under `scripts/fixtures/capability-governance/valid/` as protected byte-for-byte inputs.
- `scripts/capability-governance-smoke.sh` and `Cargo.lock` already contain uncommitted user work. Re-read their live bytes before editing. Task 2 uses the resetless alternate-index protocol below; ordinary commits require an empty real index and exact path staging.
- A missing `/usr/bin/bwrap`, `/usr/bin/strace`, unsupported user namespaces, a Python without `jsonschema`, or an empty Cargo cache is a failed prerequisite, not permission to weaken the contract.
- Do not modify `kiana-entrypoints/src/runner.rs`, product crates, release packaging, schemas, valid fixtures, or `reference/`.

## File Map

| Path | Responsibility |
| --- | --- |
| `.planning/phases/01-baseline-evidence-governance/01-03-PLAN.md` | Approved Rust remediation contract and focused verification commands |
| `.planning/phases/01-baseline-evidence-governance/01-03-SUMMARY.md` | Blocked state first; final evidence only after every gate passes |
| `Cargo.toml` | Add the standalone workspace member |
| `Cargo.lock` | Add only the supervisor package relation through an alternate index |
| `kiana-capability-governance-supervisor/Cargo.toml` | Private binary crate metadata; `std + libc` only |
| `kiana-capability-governance-supervisor/build.rs` | Canonical Python discovery, metadata validation, isolated positive/negative `jsonschema` canary, compile-time binding |
| `kiana-capability-governance-supervisor/src/lib.rs` | Typed protocol, parsers, trusted paths, bounded resources, test seam, Linux adapter, orchestration, cleanup, and unit/fake tests |
| `kiana-capability-governance-supervisor/src/main.rs` | Public/internal argv dispatch and stable exit-code mapping only |
| `kiana-capability-governance-supervisor/tests/supervisor_linux.rs` | Real fixed-launcher Linux/WSL integration and hostile-runner probes |
| `scripts/capability-governance-smoke.sh` | Public one-slice dispatcher plus hidden semantic worker; no supervisor authority |
| `scripts/capability-governance-supervisor-smoke.sh` | Independent public/adversarial regression gate |

Use these names throughout all tasks:

```rust
pub const SUPERVISOR_WORKER_EXIT_CODE: i32 = 80;
pub const MAX_STDOUT_BYTES: usize = 1 << 20;
pub const MAX_STDERR_BYTES: usize = 1 << 20;
pub const MAX_TRACE_BYTES: usize = 64 << 10;
pub const MAX_RECEIPT_BYTES: usize = 256;
pub const SLICE_DEADLINE: Duration = Duration::from_secs(30);
pub const TERM_GRACE: Duration = Duration::from_secs(2);

pub enum Slice { Schemas, FixtureShapes }
pub struct LaunchToken([u8; 32]);
pub struct SupervisorConfig {
    pub repo_root: PathBuf,
    pub runner: PathBuf,
    pub supervisor_bin: PathBuf,
    pub deadline: Duration,
    pub term_grace: Duration,
    pub limits: CaptureLimits,
}
pub trait ProcessLauncher {
    fn spawn(
        &self,
        spec: &SandboxLaunchSpec,
        channels: ChildChannels,
    ) -> Result<Box<dyn RunningProcess>, SupervisorError>;
}
pub struct LinuxProcessLauncher;
pub fn run_public(
    config: &SupervisorConfig,
    slice: Slice,
    launcher: &dyn ProcessLauncher,
) -> SupervisorOutcome;
```

### Task 1: Demote False Completion and Replace the 01-03 Contract

**Files:**
- Modify: `.planning/phases/01-baseline-evidence-governance/01-03-PLAN.md`
- Modify: `.planning/phases/01-baseline-evidence-governance/01-03-SUMMARY.md`

- [ ] **Step 1: Capture protected bytes and record the stale runtime contract**

```bash
test -z "$(git diff --cached --name-only)"
sha256sum \
  .planning/config.json \
  scripts/schema-contract-smoke.sh \
  scripts/validate-json-schema.py \
  scripts/fixtures/capability-governance/valid/*.json \
  > /tmp/kiana-01-03-protected.before

python3 - <<'PY'
from pathlib import Path

plan = Path(".planning/phases/01-baseline-evidence-governance/01-03-PLAN.md").read_text()
summary = Path(".planning/phases/01-baseline-evidence-governance/01-03-SUMMARY.md").read_text()
assert "status: blocked" in summary
assert "kiana-capability-governance-supervisor" in plan
assert "without a package or Cargo build" not in plan
PY
```

Expected: the assertions confirm that the stale runtime contract has already been demoted and replaced before production-code changes begin.

- [ ] **Step 2: Demote the summary before production-code changes**

Set summary frontmatter to `status: blocked`. Add this notice near the top:

```markdown
## Runtime Authority Remediation

**Status:** blocked. Commit `a9e4a11` proves the Shell supervisor can receive an
unknown trace-parser status and still publish success. All runtime guard, receipt,
signal-cleanup, and fresh-runtime pass claims below are superseded until the Rust
supervisor design is implemented and independently verified. Fixture contents and
semantic schema assertions remain valid evidence. Phase 01-04 must not start.
```

Mark every timeout/trace/receipt/signal/fresh-runtime pass claim `superseded`; retain fixture and schema-semantic evidence.

- [ ] **Step 3: Replace the PLAN runtime contract**

Update `files_modified` to the File Map paths. List the four fixtures as protected verification inputs, not modified outputs. Replace the stale truth with:

```yaml
must_haves:
  truths:
    - "D-01..D-21: the four valid fixtures preserve the approved governance semantics byte-for-byte."
    - "D-22/D-24: a locked/offline-built Rust supervisor is the sole runtime success authority; each prebuilt slice completes below 30 seconds."
    - "Unknown launcher, parser, worker, receipt, output, process, or cleanup state fails closed and cannot publish offline=true."
```

Replace no-Cargo verification with:

```bash
env -u CARGO_BUILD_TARGET \
  cargo build --target-dir "$PWD/target" \
  -p kiana-capability-governance-supervisor --locked --offline
bash scripts/capability-governance-supervisor-smoke.sh
bash scripts/capability-governance-smoke.sh schemas
bash scripts/capability-governance-smoke.sh fixture-shapes
```

State the prerequisites: Rust 1.96, warmed Cargo cache, fixed Linux launchers, and an isolated-mode Python with `jsonschema` available at build time.

- [ ] **Step 4: Verify the updated docs and protected bytes**

```bash
python3 - <<'PY'
from pathlib import Path

plan = Path(".planning/phases/01-baseline-evidence-governance/01-03-PLAN.md").read_text()
summary = Path(".planning/phases/01-baseline-evidence-governance/01-03-SUMMARY.md").read_text()
assert "status: blocked" in summary
assert "a9e4a11" in summary
assert "Phase 01-04 must not start" in summary
assert "kiana-capability-governance-supervisor" in plan
assert "--locked --offline" in plan
assert "without a package or Cargo build" not in plan
PY
sha256sum -c /tmp/kiana-01-03-protected.before
git diff --check -- \
  .planning/phases/01-baseline-evidence-governance/01-03-PLAN.md \
  .planning/phases/01-baseline-evidence-governance/01-03-SUMMARY.md
```

- [ ] **Step 5: Commit only the demotion and contract sync**

```bash
git add -- \
  .planning/phases/01-baseline-evidence-governance/01-03-PLAN.md \
  .planning/phases/01-baseline-evidence-governance/01-03-SUMMARY.md
expected=$(printf '%s\n' \
  .planning/phases/01-baseline-evidence-governance/01-03-PLAN.md \
  .planning/phases/01-baseline-evidence-governance/01-03-SUMMARY.md | sort)
test "$(git diff --cached --name-only | sort)" = "$expected"
git commit -m "docs(01-03): demote runtime claims for supervisor remediation"
test -z "$(git diff --cached --name-only)"
```

### Task 2: Scaffold the Crate and Bind a Trusted Python at Build Time

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock` using the alternate-index protocol only
- Create: `kiana-capability-governance-supervisor/Cargo.toml`
- Create: `kiana-capability-governance-supervisor/build.rs`
- Create: `kiana-capability-governance-supervisor/src/lib.rs`
- Create: `kiana-capability-governance-supervisor/src/main.rs`

- [ ] **Step 1: Add the private workspace crate**

Add only `"kiana-capability-governance-supervisor"` to root `workspace.members`. Create:

```toml
[package]
name = "kiana-capability-governance-supervisor"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true
publish = false
build = "build.rs"

[dependencies]
libc = "0.2"
```

`src/lib.rs` initially exports:

```rust
pub const EMBEDDED_PYTHON: &str = env!("KIANA_GOVERNANCE_PYTHON");
```

`src/main.rs` initially prints `supervisor_unavailable` to stderr and exits `1`; it must not report success before the authority model exists.

- [ ] **Step 2: Implement the std-only build binding**

Implement these `build.rs` functions:

```rust
fn discover_python() -> Result<PathBuf, String>;
fn validate_python(path: &Path) -> Result<PathBuf, String>;
fn run_jsonschema_canary(path: &Path) -> Result<(), String>;
```

`discover_python()` searches PATH components for `python3`, then `python`, without calling `which`. `validate_python()` canonicalizes, requires a regular executable owned by raw-FFI `getuid()` or root, rejects other-write, and allows group-write only when interpreter gid equals raw-FFI `getgid()`. `run_jsonschema_canary()` runs `<python> -I -c <fixed script>`; the script validates one Draft 2020-12 positive instance and proves one wrong-typed instance raises `jsonschema.ValidationError`.

`main()` must emit only after all checks pass:

```rust
println!("cargo:rerun-if-env-changed=PATH");
println!(
    "cargo:rustc-env=KIANA_GOVERNANCE_PYTHON={}",
    canonical_python.display()
);
```

Do not read `PYTHONPATH`, `PYTHONHOME`, or a runtime override variable.

- [ ] **Step 3: Add build-script focused verification cases**

Add build-script unit tests named `discover_python_prefers_python3_and_canonicalizes_it`, `validate_python_rejects_other_writable_executable`, and `jsonschema_canary_requires_positive_accept_and_negative_reject`. The first puts distinct mode-`0755` `python3` and `python` executables in one mode-`0700` test PATH directory and asserts the canonical `python3` path. The second changes a valid fixture from `0755` to `0757` and asserts `validate_python()` returns the other-write error. The third uses one fake interpreter that returns nonzero for the fixed canary and one supported real interpreter; it asserts the fake is rejected and the real positive/negative Draft 2020-12 canary passes. A test-root guard removes every path on drop.

- [ ] **Step 4: Run build-script verification and update the live lock without dropping user hunks**

```bash
rustc --edition=2021 --test kiana-capability-governance-supervisor/build.rs \
  -o /tmp/kiana-supervisor-build-tests
/tmp/kiana-supervisor-build-tests --nocapture
cp Cargo.lock /tmp/kiana-supervisor-live-lock.before
env -u CARGO_BUILD_TARGET \
  cargo check --target-dir "$PWD/target" \
  -p kiana-capability-governance-supervisor --offline
env -u CARGO_BUILD_TARGET \
  cargo build --target-dir "$PWD/target" \
  -p kiana-capability-governance-supervisor --locked --offline
```

Expected: focused build-script checks pass; the unlocked check adds the new package relation on top of live user hunks; the locked build passes.

- [ ] **Step 5: Derive a supervisor-only lock blob from HEAD**

```bash
start_head=$(git rev-parse HEAD)
phase_index=$(mktemp /tmp/kiana-supervisor-index.XXXXXX)
rm -f "$phase_index"
phase_tree_dir=$(mktemp -d /tmp/kiana-supervisor-lock-tree.XXXXXX)
GIT_INDEX_FILE="$phase_index" git read-tree "$start_head"

for path in \
  Cargo.toml \
  kiana-capability-governance-supervisor/Cargo.toml \
  kiana-capability-governance-supervisor/build.rs \
  kiana-capability-governance-supervisor/src/lib.rs \
  kiana-capability-governance-supervisor/src/main.rs
do
  blob=$(git hash-object -w "$path")
  GIT_INDEX_FILE="$phase_index" git update-index \
    --add --cacheinfo "100644,$blob,$path"
done

prelock_tree=$(GIT_INDEX_FILE="$phase_index" git write-tree)
git archive "$prelock_tree" | tar -x -C "$phase_tree_dir"
(
  cd "$phase_tree_dir"
  env -u CARGO_BUILD_TARGET cargo check --target-dir "$phase_tree_dir/target" \
    -p kiana-capability-governance-supervisor --offline
)

git show "$start_head:Cargo.lock" > /tmp/kiana-supervisor-lock.base
cp "$phase_tree_dir/Cargo.lock" /tmp/kiana-supervisor-lock.expected
git diff --no-index -- \
  /tmp/kiana-supervisor-lock.base \
  /tmp/kiana-supervisor-lock.expected \
  > /tmp/kiana-supervisor-lock.patch || test $? -eq 1
rg -n 'kiana-capability-governance-supervisor|libc' \
  /tmp/kiana-supervisor-lock.patch
```

Inspect the patch. Stop if it changes package versions or relations unrelated to the supervisor.

- [ ] **Step 6: Prove the live lock equals original user hunks plus the supervisor hunk**

```bash
git merge-file -p \
  /tmp/kiana-supervisor-live-lock.before \
  /tmp/kiana-supervisor-lock.base \
  /tmp/kiana-supervisor-lock.expected \
  > /tmp/kiana-supervisor-lock.combined
cmp /tmp/kiana-supervisor-lock.combined Cargo.lock
```

Expected: merge exits `0` without conflict and `cmp` passes. A conflict or mismatch is a hard stop.

- [ ] **Step 7: Commit through the alternate index and reconcile only committed paths in the real index**

```bash
lock_blob=$(git hash-object -w /tmp/kiana-supervisor-lock.expected)
GIT_INDEX_FILE="$phase_index" git update-index \
  --cacheinfo "100644,$lock_blob,Cargo.lock"

expected_paths=$(printf '%s\n' \
  Cargo.lock \
  Cargo.toml \
  kiana-capability-governance-supervisor/Cargo.toml \
  kiana-capability-governance-supervisor/build.rs \
  kiana-capability-governance-supervisor/src/lib.rs \
  kiana-capability-governance-supervisor/src/main.rs | sort)
test "$(GIT_INDEX_FILE="$phase_index" git diff --cached \
  --name-only "$start_head" | sort)" = "$expected_paths"
test "$(git rev-parse HEAD)" = "$start_head"
GIT_INDEX_FILE="$phase_index" git commit \
  -m "feat(01-03): scaffold governance supervisor and bind python"

new_head=$(git rev-parse HEAD)
test "$(git rev-parse "$new_head^")" = "$start_head"
test "$(git diff-tree --no-commit-id --name-only -r "$new_head" | sort)" = \
  "$expected_paths"

for path in $expected_paths
do
  entry=$(git ls-tree "$new_head" -- "$path")
  mode=${entry%% *}
  rest=${entry#* }
  object_and_path=${rest#blob }
  object=${object_and_path%%$'\t'*}
  git update-index --add --cacheinfo "$mode,$object,$path"
done

test -z "$(git diff --cached --name-only)"
test "$(git diff --name-only -- Cargo.lock)" = Cargo.lock
git diff -- Cargo.lock
rm -rf "$phase_index" "$phase_tree_dir"
```

The final diff must contain only the original user lock hunks relative to new HEAD.

### Task 3: Define Typed Parsers and the Unique Success Predicate

**Files:**
- Modify: `kiana-capability-governance-supervisor/src/lib.rs`

- [ ] **Step 1: Add the complete fail-closed state model**

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeadlineState { Pending, Completed, Exceeded, Interrupted(i32), Unknown }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SandboxState { NotStarted, Started, Failed, Unknown }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerExit { Code(i32), Signaled(i32), Missing, Unknown }

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TraceChannelState {
    ParentOwnedAndWorkerInaccessible,
    WorkerVisible,
    Tampered,
    Missing,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TraceVerdict {
    ExactStartupCanaryOnly,
    NetworkAttempt { syscall: String },
    Invalid,
    Missing,
    Oversized,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReceiptVerdict {
    ExactAndEof,
    Missing,
    Early,
    Duplicate,
    WrongToken,
    WrongSlice,
    TrailingBytes,
    Oversized,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureVerdict {
    CompleteBounded,
    LimitExceeded,
    Truncated,
    ReadFailed,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessTreeState { FullyReaped, DescendantsRemain, GroupKillFailed, Unknown }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerTmpState { Empty, NonEmpty, Missing, Unknown }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CleanupState { Removed, Residue, Failed, Unknown }

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FailureCode {
    SupervisorUnavailable,
    LauncherUntrusted,
    SandboxStartFailed,
    SandboxAttestationFailed,
    NetworkAttempt { syscall: String },
    DeadlineExceeded,
    WorkerFailed,
    ReceiptInvalid,
    OutputLimitExceeded,
    ProcessCleanupFailed,
    RuntimeCleanupFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UsageError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SupervisorError {
    pub code: FailureCode,
    pub context: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Verdict {
    pub launchers_trusted: bool,
    pub sandbox: SandboxState,
    pub deadline: DeadlineState,
    pub worker_exit: WorkerExit,
    pub trace_channel: TraceChannelState,
    pub trace: TraceVerdict,
    pub receipt: ReceiptVerdict,
    pub stdout: CaptureVerdict,
    pub stderr: CaptureVerdict,
    pub stderr_empty: bool,
    pub worker_stdout_has_final_marker: bool,
    pub process_tree: ProcessTreeState,
    pub worker_tmp: WorkerTmpState,
    pub runtime_cleanup: CleanupState,
}
```

No `Default` implementation may produce success. Keep the all-success constructor under `#[cfg(test)]`.

- [ ] **Step 2: Implement exact slice/token/receipt/trace parsing**

```rust
impl Slice {
    pub fn parse_public(value: &str) -> Result<Self, UsageError>;
    pub const fn as_str(&self) -> &'static str;
    pub const fn worker_arg(&self) -> &'static str;
}

impl LaunchToken {
    pub fn generate() -> io::Result<Self>;
    pub fn as_hex(&self) -> String;
}

pub fn parse_receipt(
    bytes: &[u8],
    token: &LaunchToken,
    slice: Slice,
    observed_before_worker_exit: bool,
) -> ReceiptVerdict;

pub fn parse_trace(bytes: &[u8]) -> TraceVerdict;

impl Verdict {
    pub fn is_authorized_success(&self) -> bool;
    pub fn failure_code(&self) -> Option<FailureCode>;
}
```

`LaunchToken::generate()` uses `libc::SYS_getrandom`, retries `EINTR`, and requires 32 bytes. `parse_receipt()` returns `Early` when `observed_before_worker_exit` is true; otherwise it requires exactly one `complete:<64 lowercase hex>:<slice>\n` followed by EOF. The receipt reader records the timing bit when it observes its first byte, using the same child-exited atomic updated by the wait loop. `parse_trace()` accepts ASCII only, allows no unparsed non-empty line, and requires the first and only syscall to be injected `socket` returning `EPERM`; any additional `%network` or `io_uring_*` record returns `NetworkAttempt`.

`is_authorized_success()` is one positive conjunction. `worker_exit` must equal `Code(80)`, `stderr` must equal `CompleteBounded`, and `stderr_empty` must be true. Unknown and future non-success variants return false.

- [ ] **Step 3: Add table-driven focused and adversarial verification**

Add unit tests:

```text
verdict_only_all_true_authorizes_success
verdict_each_required_component_failure_blocks_success
verdict_worker_exit_zero_fails_closed
verdict_unknown_states_fail_closed
parse_receipt_accepts_exact_token_slice_eof
parse_receipt_rejects_missing_early_duplicate_wrong_token_wrong_slice_trailing_and_oversized
parse_trace_accepts_exact_startup_canary_only
parse_trace_rejects_extra_network_and_io_uring_attempts
parse_trace_redacts_pid_args_and_invalid_utf8
```

The verdict test starts from one all-success fixture and mutates launcher, sandbox, deadline, worker, trace channel, trace, receipt, stdout, stderr status, stderr emptiness, final-marker, process-tree, worker-tmp, and cleanup fields one at a time. Every mutation returns false. The receipt table calls the parser once with `observed_before_worker_exit=true` and requires `Early` even when the bytes are otherwise exact.

- [ ] **Step 4: Verify and commit**

```bash
env -u CARGO_BUILD_TARGET cargo test --target-dir "$PWD/target" \
  -p kiana-capability-governance-supervisor --locked --offline \
  --lib --no-fail-fast
git diff --check -- kiana-capability-governance-supervisor/src/lib.rs
git add -- kiana-capability-governance-supervisor/src/lib.rs
test "$(git diff --cached --name-only)" = \
  kiana-capability-governance-supervisor/src/lib.rs
git commit -m "feat(01-03): define fail-closed supervisor verdict contract"
test -z "$(git diff --cached --name-only)"
```

### Task 4: Add Trusted Paths, Runtime Root, FD, and Capture Primitives

**Files:**
- Modify: `kiana-capability-governance-supervisor/src/lib.rs`

- [ ] **Step 1: Implement trusted launchers and embedded Python preflight**

```rust
pub enum LauncherKind { Bwrap, Strace, Bash, Shell, Python }

pub struct FixedLauncherPaths {
    pub bwrap: PathBuf,
    pub strace: PathBuf,
    pub bash: PathBuf,
    pub shell: PathBuf,
}

pub struct TrustedExecutable {
    pub path: PathBuf,
    pub uid: u32,
    pub gid: u32,
    pub mode: u32,
}

pub struct TrustedLaunchers {
    pub bwrap: TrustedExecutable,
    pub strace: TrustedExecutable,
    pub bash: TrustedExecutable,
    pub shell: TrustedExecutable,
    pub python: TrustedExecutable,
}

pub fn validate_fixed_launchers(
    paths: &FixedLauncherPaths,
) -> Result<TrustedLaunchers, SupervisorError>;

pub fn embedded_python_path() -> Result<TrustedExecutable, SupervisorError>;
```

`bwrap`, `strace`, and Bash must canonicalize to their fixed paths; canonical `/bin/sh` may differ. System launchers are regular root-owned executables with no group/other write. Embedded Python revalidates the exact compiled path and repeats the fixed `-I` canary. No runtime PATH fallback exists.

- [ ] **Step 2: Implement runtime root and launch channels**

```rust
pub struct CaptureLimits {
    pub stdout: usize,
    pub stderr: usize,
    pub trace: usize,
    pub receipt: usize,
}

pub struct RuntimeRoot {
    path: PathBuf,
    worker_tmp: PathBuf,
}

impl RuntimeRoot {
    pub fn create() -> Result<Self, SupervisorError>;
    pub fn path(&self) -> &Path;
    pub fn worker_tmp(&self) -> &Path;
    pub fn inspect_worker_tmp(&self) -> WorkerTmpState;
    pub fn cleanup(self) -> CleanupState;
}

pub struct ParentChannels {
    pub launch_write: OwnedFd,
    pub completion_read: OwnedFd,
    pub worker_stdout_read: OwnedFd,
    pub worker_stderr_read: OwnedFd,
    pub guard_stderr_read: OwnedFd,
    pub trace_read: OwnedFd,
}

pub struct ChildChannels {
    pub launch_read: OwnedFd,
    pub completion_write: OwnedFd,
    pub worker_stdout_write: OwnedFd,
    pub worker_stderr_write: OwnedFd,
    pub guard_stderr_write: OwnedFd,
    pub trace_write: OwnedFd,
}

pub struct ChannelSet {
    pub parent: ParentChannels,
    pub child: ChildChannels,
}

impl ChannelSet {
    pub fn create() -> Result<Self, SupervisorError>;
}
```

Create `/tmp/kiana-capability-governance-<64hex>` with a second `getrandom` value independent of the launch token, create-if-absent semantics, mode `0700`, and current ownership. Ignore caller `TMPDIR`; reject collision, symlink, mode, or ownership mismatch. Create independent `pipe2(O_CLOEXEC)` pairs. Clear `CLOEXEC` only on intentional child descriptors. Close unexpected FDs with `close_range` and an `RLIMIT_NOFILE` bounded fallback.

- [ ] **Step 3: Implement concurrent bounded capture**

```rust
pub struct BoundedCapture {
    pub bytes: Vec<u8>,
    pub verdict: CaptureVerdict,
}

pub fn read_bounded<R: Read + Send + 'static>(
    reader: R,
    limit: usize,
) -> JoinHandle<BoundedCapture>;
```

Store at most `limit` bytes, read one sentinel byte to detect overflow, close the endpoint on overflow, and never label truncated data complete. Orchestration starts stdout, stderr, guard, trace, and receipt readers before waiting.

- [ ] **Step 4: Add focused boundary verification**

Add:

```text
launcher_validation_rejects_nonroot_writable_and_noncanonical_objects
embedded_python_ignores_runtime_path_and_rechecks_canary
runtime_root_ignores_caller_tmpdir_and_is_mode_0700
runtime_root_rejects_existing_path_and_symlink
bounded_capture_accepts_exact_limit_and_rejects_limit_plus_one
close_unexpected_fds_keeps_only_declared_descriptors
```

Negative launcher tests use test-owned files. The supported-host positive test inspects fixed system paths. Capture limits are exactly 1 MiB stdout/stderr, 64 KiB trace, and 256 bytes receipt.

- [ ] **Step 5: Verify and commit**

```bash
env -u CARGO_BUILD_TARGET cargo test --target-dir "$PWD/target" \
  -p kiana-capability-governance-supervisor --locked --offline \
  --lib --no-fail-fast
git diff --check -- kiana-capability-governance-supervisor/src/lib.rs
git add -- kiana-capability-governance-supervisor/src/lib.rs
test "$(git diff --cached --name-only)" = \
  kiana-capability-governance-supervisor/src/lib.rs
git commit -m "feat(01-03): add trusted runtime and bounded IO primitives"
test -z "$(git diff --cached --name-only)"
```

### Task 5: Build the Test-Only Process Seam and Fake Failure Harness

**Files:**
- Modify: `kiana-capability-governance-supervisor/src/lib.rs`

- [ ] **Step 1: Define the launch seam**

```rust
pub struct SandboxLaunchSpec {
    pub bwrap_argv: Vec<OsString>,
    pub strace_argv: Vec<OsString>,
    pub worker_argv: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
    pub runtime_root: PathBuf,
    pub worker_tmp: PathBuf,
}

pub trait RunningProcess {
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>>;
    fn terminate_group(&self, signal: i32) -> io::Result<()>;
    fn wait(&mut self) -> io::Result<ExitStatus>;
    fn process_group(&self) -> libc::pid_t;
}

pub trait ProcessLauncher {
    fn spawn(
        &self,
        spec: &SandboxLaunchSpec,
        channels: ChildChannels,
    ) -> Result<Box<dyn RunningProcess>, SupervisorError>;
}

pub struct SupervisorOutcome {
    pub exit_code: i32,
    pub public_stdout: Vec<u8>,
    pub public_stderr: Vec<u8>,
    pub verdict: Verdict,
}
```

`FakeProcessLauncher` and fake process controls live only inside `#[cfg(test)]`. Production `main.rs` constructs only `LinuxProcessLauncher`.

- [ ] **Step 2: Add a strict orchestration skeleton**

Add `run_public(config, slice, launcher)`. It validates trusted objects, creates token/root/channels, calls the launcher, captures results, builds every `Verdict` field explicitly, cleans the root, and calls `is_authorized_success()` once. Deadline/signal states not yet produced remain `Unknown`, so no success is possible in this task.

No environment variable, CLI flag, or alternate filesystem path may select a fake launcher.

- [ ] **Step 3: Implement the fake-process harness and adversarial verification**

Add unit tests:

```text
fake_noop_launcher_exit_zero_does_not_authorize
fake_unknown_launcher_exit_fails_closed
fake_receipt_variants_fail_closed
fake_output_and_trace_limits_trigger_cleanup
fake_worker_fd_trace_forgery_cannot_mutate_parent_capture
fake_leaked_guard_or_trace_fd_fails_attestation
fake_deadline_hang_and_term_resistant_descendant_are_reaped
fake_runtime_cleanup_failure_overrides_success
```

Each fake scenario creates actual child processes and pipes. It may not directly inject an all-success `Verdict`.

- [ ] **Step 4: Verify fake failures and commit**

```bash
env -u CARGO_BUILD_TARGET cargo test --target-dir "$PWD/target" \
  -p kiana-capability-governance-supervisor --locked --offline \
  fake_ -- --nocapture
git diff --check -- kiana-capability-governance-supervisor/src/lib.rs
git add -- kiana-capability-governance-supervisor/src/lib.rs
test "$(git diff --cached --name-only)" = \
  kiana-capability-governance-supervisor/src/lib.rs
git commit -m "test(01-03): add adversarial supervisor process harness"
test -z "$(git diff --cached --name-only)"
```

Expected: every adversarial scenario returns nonzero and no final marker.

### Task 6: Move Bash Semantics Behind the Hidden Worker Protocol

**Files:**
- Modify: `scripts/capability-governance-smoke.sh`

- [ ] **Step 1: Capture the current Shell-authority baseline**

```bash
runner=scripts/capability-governance-smoke.sh
test "$(git ls-files -s "$runner" | awk '{print $1}')" = 100755

set +e
bash "$runner" --internal-worker schemas \
  >/tmp/kiana-worker.out 2>/tmp/kiana-worker.err
direct_status=$?
set -e
test "$direct_status" -ne 0
! rg -q 'offline=true' /tmp/kiana-worker.out /tmp/kiana-worker.err

rg -n \
  'run_guarded_worker|validate_startup_attestation|command -v timeout|command -v strace' \
  "$runner"
```

Expected: the baseline records that direct worker is nonzero while Shell still owns deadline/trace authority; the next steps remove that ownership.

- [ ] **Step 2: Add strict public/hidden dispatch**

The public branch accepts exactly one known slice, checks only `<repo>/target/debug/kiana-capability-governance-supervisor`, and `exec`s it. Missing/unknown usage is `2`; missing/non-executable helper is `1` with:

```text
supervisor_unavailable: build kiana-capability-governance-supervisor with --locked --offline
```

The hidden branch accepts exactly:

```text
scripts/capability-governance-smoke.sh --internal-worker {schemas|fixture-shapes}
```

No public branch performs an implicit build, PATH lookup, timeout, strace, trace parsing, receipt parsing, or success printing.

- [ ] **Step 3: Implement the inherited-FD worker handshake**

The hidden worker validates distinct numeric `KIANA_GOVERNANCE_LAUNCH_FD` and `KIANA_GOVERNANCE_COMPLETION_FD`, then reads one exact 64-lowercase-hex token line followed by EOF. It must use the exact `KIANA_GOVERNANCE_PYTHON` path with `-I`, not `python_bin()` or PATH.

Run the startup socket canary before any semantic function. Keep existing `run_schema_slice()` and `run_fixture_shape_slice()` bodies. On success:

```bash
printf 'complete:%s:%s\n' "$launch_token" "$slice" >&"$completion_fd"
exec {completion_fd}>&-
exit 80
```

Remove Shell parent supervision, external `timeout`, trace files/parsers, completion FIFO, review/probe short-circuits, caller-authentication language, and every worker `offline=true` print.

- [ ] **Step 4: Verify hidden-worker protocol**

Use a short Python subprocess harness with `pass_fds` to prove: exact token gives one receipt plus exit `80`; missing/wrong/duplicate token fails; unknown slice fails; direct worker fails; no case prints the final marker. Then run:

```bash
! rg -n \
  'run_guarded_worker|validate_startup_attestation|command -v timeout|command -v strace|KIANA_CAPABILITY_GOVERNANCE_REVIEW_TEST' \
  "$runner"
test "$(git ls-files -s "$runner" | awk '{print $1}')" = 100755
git diff --check -- "$runner"
```

- [ ] **Step 5: Commit only the worker migration**

```bash
git add -- scripts/capability-governance-smoke.sh
test "$(git diff --cached --name-only)" = \
  scripts/capability-governance-smoke.sh
git commit -m "feat(01-03): migrate governance semantics behind hidden worker"
test -z "$(git diff --cached --name-only)"
```

### Task 7: Implement the Fixed Linux Adapter and Trusted Internal Modes

**Files:**
- Modify: `kiana-capability-governance-supervisor/src/lib.rs`
- Modify: `kiana-capability-governance-supervisor/src/main.rs`
- Create: `kiana-capability-governance-supervisor/tests/supervisor_linux.rs`

- [ ] **Step 1: Implement public/internal invocation parsing**

```rust
pub enum Invocation {
    Public(Slice),
    TraceLogger,
    WorkerLauncher { runner: PathBuf, slice: Slice },
}

pub fn parse_invocation(args: &[OsString]) -> Result<Invocation, UsageError>;
```

Accept exactly:

```text
<binary> {schemas|fixture-shapes}
<binary> __trace-logger
<binary> __worker-launcher --runner <canonical-runner> --slice {schemas|fixture-shapes}
```

Public usage errors map to `2`; internal misuse maps to `1`. No debug success route exists.

- [ ] **Step 2: Build the fixed sandbox plan**

Implement:

```rust
pub fn build_sandbox_launch_spec(
    config: &SupervisorConfig,
    channels: &ChildChannels,
    token: &LaunchToken,
    slice: Slice,
) -> Result<SandboxLaunchSpec, SupervisorError>;
```

Construct all argv/env as separate `OsString` values. The strace segment is exactly:

```text
/usr/bin/strace -f -qq
-e trace=%network,io_uring_setup,io_uring_enter,io_uring_register
-e signal=none
-e inject=%network:error=EPERM
-e inject=io_uring_setup:error=EPERM
-e inject=io_uring_enter:error=EPERM
-e inject=io_uring_register:error=EPERM
-o |/tmp/kiana-supervisor __trace-logger
-- /tmp/kiana-supervisor __worker-launcher --runner <runner> --slice <slice>
```

The pipe command is a constant with no repo, slice, token, or caller interpolation. The bwrap environment is cleared and restores only fixed PATH/locale, private home/tmp, embedded Python, and intentional FD numbers.

- [ ] **Step 3: Implement `LinuxProcessLauncher` and internal helpers**

Spawn fixed `/usr/bin/bwrap`. `pre_exec` performs only async-signal-safe process-group and FD operations. `trace_logger_main()` copies strace-owned stdin to stdout and never opens a file/network path. `worker_launcher_main()` applies `PR_SET_NO_NEW_PRIVS`, clears capabilities, redirects worker stdout/stderr, opens `/dev/null` for stdin, closes trace/guard/unexpected FDs, and `execve()`s:

```text
/usr/bin/bash <canonical-runner> --internal-worker <slice>
```

Never use `bash -c`.

- [ ] **Step 4: Add prerequisite-aware Linux test setup**

`tests/supervisor_linux.rs` starts with `#![cfg(target_os = "linux")]` and uses `env!("CARGO_BIN_EXE_kiana-capability-governance-supervisor")`. A shared check validates fixed launchers, user namespaces, strace pipe support, and embedded Python. It may print `SKIP prerequisite: ...` for local diagnosis, but the independent/final gate must detect any skip and fail rather than restore completion.

- [ ] **Step 5: Add focused fixed-argv and internal-mode verification**

Add:

```text
sandbox_plan_has_exact_fixed_argv_and_no_caller_interpolation
sandbox_plan_injects_all_network_and_io_uring_calls
trace_logger_copies_stdin_only_to_stdout
worker_launcher_sets_no_new_privs_redirects_and_closes_fds
worker_cannot_open_proc_or_dev_fd_trace_channel
fixed_launchers_reject_path_and_loader_shims
```

The argv test compares the complete vector: `--die-with-parent`, `--unshare-all`, `--clearenv`, read-only `/`, private `/dev`, empty read-only `/proc`, private `/tmp`, fixed home, read-only supervisor bind, worker tmp bind, canonical repo cwd, fixed environment, and the fixed trace-logger pipe command.

- [ ] **Step 6: Verify and commit**

```bash
env -u CARGO_BUILD_TARGET cargo test --target-dir "$PWD/target" \
  -p kiana-capability-governance-supervisor --locked --offline \
  sandbox_plan_ -- --nocapture
env -u CARGO_BUILD_TARGET cargo test --target-dir "$PWD/target" \
  -p kiana-capability-governance-supervisor --locked --offline \
  trace_logger_ -- --nocapture
git diff --check -- \
  kiana-capability-governance-supervisor/src/lib.rs \
  kiana-capability-governance-supervisor/src/main.rs \
  kiana-capability-governance-supervisor/tests/supervisor_linux.rs
git add -- \
  kiana-capability-governance-supervisor/src/lib.rs \
  kiana-capability-governance-supervisor/src/main.rs \
  kiana-capability-governance-supervisor/tests/supervisor_linux.rs
expected=$(printf '%s\n' \
  kiana-capability-governance-supervisor/src/lib.rs \
  kiana-capability-governance-supervisor/src/main.rs \
  kiana-capability-governance-supervisor/tests/supervisor_linux.rs | sort)
test "$(git diff --cached --name-only | sort)" = "$expected"
git commit -m "feat(01-03): implement fixed Linux sandbox adapter"
test -z "$(git diff --cached --name-only)"
```

### Task 8: Wire Deadline, Signals, Bounded Drains, Cleanup, and Publication

**Files:**
- Modify: `kiana-capability-governance-supervisor/src/lib.rs`
- Modify: `kiana-capability-governance-supervisor/src/main.rs`

- [ ] **Step 1: Implement one bounded wait/cleanup state machine**

Add `run_deadline_loop()` and `cleanup_process_group()`. Use `clock_gettime(CLOCK_MONOTONIC)` and a `sigaction` handler that only stores the signal in an atomic. After spawn:

1. close parent copies of child write ends and write the launch token once;
2. start all bounded readers before waiting;
3. poll `try_wait()`, signal state, reader overflow/error, and the 30-second deadline;
4. on every abnormal path send TERM to `-pgid`, wait at most two monotonic seconds, then KILL;
5. wait direct child, join readers, require receipt EOF, inspect worker tmp, remove runtime root, and verify absence;
6. construct every `Verdict` field explicitly and call `is_authorized_success()` once.

No return is allowed between runtime-root creation and cleanup verification.

- [ ] **Step 2: Implement stable redacted publishing**

Implement `FailureCode::render_diagnostic()`. It emits only stable codes; network may include a validated syscall name. It excludes `SupervisorError::context`, raw trace, token, PID, home/runtime paths, and syscall arguments.

Only after full authority and confirmed cleanup may Rust publish quarantined worker stdout and append exactly one:

```text
OK: slice=<schemas|fixture-shapes> elapsed_seconds=<bounded decimal> offline=true
```

Worker stderr is empty on success. A bounded semantic diagnostic may be released only after sandbox/trace attestation and must reject final markers, 64-hex values, and absolute/transient paths.

- [ ] **Step 3: Add focused orchestration verification**

```text
exact_fake_success_publishes_one_final_marker_after_cleanup
deadline_uses_monotonic_clock_and_kills_process_group
sigint_cleans_then_returns_130
sigterm_cleans_then_returns_143
term_grace_escalates_to_kill
reader_threads_prevent_pipe_backpressure_deadlock
guard_or_trace_reader_error_overrides_worker_success
semantic_diagnostics_publish_only_after_authority_attestation
authority_failure_redacts_trace_token_pid_and_paths
```

Add the exact fake success case only after the deadline, process, and cleanup facts are fully wired.

- [ ] **Step 4: Verify orchestration and commit**

```bash
env -u CARGO_BUILD_TARGET cargo test --target-dir "$PWD/target" \
  -p kiana-capability-governance-supervisor --locked --offline \
  --lib --no-fail-fast
env -u CARGO_BUILD_TARGET cargo build --target-dir "$PWD/target" \
  -p kiana-capability-governance-supervisor --locked --offline
git diff --check -- \
  kiana-capability-governance-supervisor/src/lib.rs \
  kiana-capability-governance-supervisor/src/main.rs
git add -- \
  kiana-capability-governance-supervisor/src/lib.rs \
  kiana-capability-governance-supervisor/src/main.rs
expected=$(printf '%s\n' \
  kiana-capability-governance-supervisor/src/lib.rs \
  kiana-capability-governance-supervisor/src/main.rs | sort)
test "$(git diff --cached --name-only | sort)" = "$expected"
git commit -m "feat(01-03): enforce supervisor deadline cleanup and publication"
test -z "$(git diff --cached --name-only)"
```

### Task 9: Cut Over the Public CLI and Add the Independent Adversarial Gate

**Files:**
- Modify: `scripts/capability-governance-smoke.sh`
- Create: `scripts/capability-governance-supervisor-smoke.sh`
- Modify: `kiana-capability-governance-supervisor/tests/supervisor_linux.rs`

- [ ] **Step 1: Finalize the public Bash dispatcher**

Keep only the public interface `scripts/capability-governance-smoke.sh {schemas|fixture-shapes}`. Missing/unknown input is `2`; missing helper is `1`; valid input execs only the fixed debug helper. Assert the script has no parent timeout, strace, trace path, completion FIFO, review mode, dynamic helper path, or final-marker publisher.

- [ ] **Step 2: Create the independent shell gate**

The Bash 5 fail-fast gate must verify:

1. public missing/unknown usage returns `2`;
2. missing fixed helper returns `1` and stable `supervisor_unavailable`;
3. forged PATH, `BASH_ENV`, `ENV`, Python variables, loader variables, old review/worker variables, and caller `TMPDIR` cannot replace trusted objects or create false success;
4. direct hidden worker, bad handshake, no-op launcher, worker exit `0`, receipt variants, forged marker, output overflow, and trace forgery never yield public `0`;
5. extra network/io_uring, hang, INT, TERM, trace-channel visibility, inherited FD, and residue Rust test filters pass with no skip;
6. both public slices produce one Rust-owned final marker and no raw trace/token/PID/runtime path;
7. Task 1 protected hashes still match.

- [ ] **Step 3: Add full real-Linux adversarial verification**

```text
linux_two_public_slices_succeed_after_prebuilt_binary
linux_normal_trace_has_exact_single_injected_socket_canary
linux_extra_socket_attempt_fails_with_redacted_diagnostic
linux_io_uring_attempt_fails_closed
linux_worker_cannot_read_or_write_trace_channel
linux_worker_cannot_reopen_trace_via_proc_fd_or_dev_fd
linux_unexpected_inherited_fd_is_closed
linux_worker_hang_exits_after_deadline_and_reaps_descendants
linux_sigint_returns_130_and_leaves_no_residue
linux_sigterm_returns_143_and_leaves_no_residue
linux_direct_hidden_worker_is_nonzero_and_never_publishes_final_marker
linux_runtime_env_shims_cannot_replace_trusted_launchers
linux_raw_trace_nonce_pid_and_runtime_path_are_not_public
linux_caller_tmpdir_is_ignored_and_empty_after_run
linux_worker_tmp_is_empty_after_success_and_failure
```

Generate hostile runner scripts inside test temp roots and pass them through `SupervisorConfig`; do not add production probe environment variables.

- [ ] **Step 4: Run the pre-commit focused gate**

```bash
cargo fmt --all --check
env -u CARGO_BUILD_TARGET cargo test --target-dir "$PWD/target" \
  -p kiana-capability-governance-supervisor --locked --offline --no-fail-fast
env -u CARGO_BUILD_TARGET cargo build --target-dir "$PWD/target" \
  -p kiana-capability-governance-supervisor --locked --offline
bash scripts/capability-governance-supervisor-smoke.sh
bash scripts/capability-governance-smoke.sh schemas
bash scripts/capability-governance-smoke.sh fixture-shapes
```

Expected: all pass; each prebuilt slice is below 30 seconds and prints one final marker.

- [ ] **Step 5: Build and test a scoped pre-commit archive tree**

```bash
gate_index=$(mktemp /tmp/kiana-supervisor-gate-index.XXXXXX)
rm -f "$gate_index"
GIT_INDEX_FILE="$gate_index" git read-tree HEAD

for path in \
  scripts/capability-governance-smoke.sh \
  scripts/capability-governance-supervisor-smoke.sh \
  kiana-capability-governance-supervisor/tests/supervisor_linux.rs
do
  blob=$(git hash-object -w "$path")
  mode=100644
  case "$path" in scripts/*.sh) mode=100755 ;; esac
  GIT_INDEX_FILE="$gate_index" git update-index \
    --add --cacheinfo "$mode,$blob,$path"
done

gate_tree=$(GIT_INDEX_FILE="$gate_index" git write-tree)
gate_root=$(mktemp -d /var/tmp/kiana-supervisor-scoped-XXXXXX)
git archive "$gate_tree" | tar -x -C "$gate_root"
test ! -e "$gate_root/reference"
(
  cd "$gate_root"
  env -u CARGO_BUILD_TARGET cargo build --target-dir "$gate_root/target" \
    -p kiana-capability-governance-supervisor --locked --offline
  bash scripts/capability-governance-supervisor-smoke.sh
  bash scripts/capability-governance-smoke.sh schemas
  bash scripts/capability-governance-smoke.sh fixture-shapes
)
rm -rf "$gate_index" "$gate_root"
```

This tree includes committed Tasks 1-8 plus only the three Task 9 worktree paths.

- [ ] **Step 6: Commit the cutover and independent gate**

```bash
git add -- \
  scripts/capability-governance-smoke.sh \
  scripts/capability-governance-supervisor-smoke.sh \
  kiana-capability-governance-supervisor/tests/supervisor_linux.rs
expected=$(printf '%s\n' \
  scripts/capability-governance-smoke.sh \
  scripts/capability-governance-supervisor-smoke.sh \
  kiana-capability-governance-supervisor/tests/supervisor_linux.rs | sort)
test "$(git diff --cached --name-only | sort)" = "$expected"
git commit -m "test(01-03): cut over public governance supervisor gate"
test -z "$(git diff --cached --name-only)"
```

### Task 10: Run Final Gates, Obtain Review, and Restore Completion

**Files:**
- Modify: `.planning/phases/01-baseline-evidence-governance/01-03-SUMMARY.md`
- Verify only: all implementation files and protected inputs

- [ ] **Step 1: Verify the committed code HEAD archive before SUMMARY changes**

```bash
code_head=$(git rev-parse HEAD)
archive_root=$(mktemp -d /var/tmp/kiana-supervisor-head-XXXXXX)
git archive "$code_head" | tar -x -C "$archive_root"
test ! -e "$archive_root/reference"
(
  cd "$archive_root"
  env -u CARGO_BUILD_TARGET cargo build --target-dir "$archive_root/target" \
    -p kiana-capability-governance-supervisor --locked --offline
  bash scripts/capability-governance-supervisor-smoke.sh
  bash scripts/capability-governance-smoke.sh schemas
  bash scripts/capability-governance-smoke.sh fixture-shapes
)
rm -rf "$archive_root"
test "$(git rev-parse HEAD)" = "$code_head"
```

- [ ] **Step 2: Run focused and isolated workspace gates**

```bash
cargo fmt --all --check
env -u CARGO_BUILD_TARGET cargo test --target-dir "$PWD/target" \
  -p kiana-capability-governance-supervisor --locked --offline --no-fail-fast
env -u CARGO_BUILD_TARGET cargo build --target-dir "$PWD/target" \
  -p kiana-capability-governance-supervisor --locked --offline
bash scripts/capability-governance-supervisor-smoke.sh
bash scripts/capability-governance-smoke.sh schemas
bash scripts/capability-governance-smoke.sh fixture-shapes

TEST_ROOT="$(mktemp -d /var/tmp/kiana-supervisor-gate-XXXXXX)"
trap 'rm -rf "$TEST_ROOT"' EXIT
mkdir -p "$TEST_ROOT/home" "$TEST_ROOT/tmp"
HOME="$TEST_ROOT/home" \
USERPROFILE="$TEST_ROOT/home" \
TMPDIR="$TEST_ROOT/tmp" \
CARGO_HOME=/home/shirosora/.cargo \
RUSTUP_HOME=/home/shirosora/.rustup \
cargo test --workspace --locked --offline --no-fail-fast -- --test-threads=1
```

- [ ] **Step 3: Prove protected bytes, cleanup, and commit scope**

```bash
sha256sum -c /tmp/kiana-01-03-protected.before
! pgrep -af \
  'kiana-capability-governance-supervisor|capability-governance-smoke.sh' \
  | rg -v 'pgrep -af'
! find /tmp -maxdepth 1 -name 'kiana-capability-governance-*' \
  -print -quit | rg .
git diff --check -- \
  Cargo.toml Cargo.lock \
  kiana-capability-governance-supervisor \
  scripts/capability-governance-smoke.sh \
  scripts/capability-governance-supervisor-smoke.sh \
  .planning/phases/01-baseline-evidence-governance/01-03-PLAN.md \
  .planning/phases/01-baseline-evidence-governance/01-03-SUMMARY.md
```

Inspect `git status --short`: unrelated dirty paths remain, no protected file or `01-04` file appears in supervisor commits, and `Cargo.lock` shows only its original user hunks relative to HEAD.

- [ ] **Step 4: Run two independent reviews**

Use `superpowers:requesting-code-review` over the Task 1 parent through `code_head`:

1. specification review maps every design section 5-17 item to code/tests and rejects a missing authority fact;
2. security/quality review checks unsafe FFI, pre-exec safety, FD inheritance, shell interpolation, signal races, bounded drains, cleanup dominance, output redaction, and production reachability of fake adapters.

Resolve every Critical, High, or contract-affecting finding with a focused implementation and verification commit, then repeat Steps 1-3.

- [ ] **Step 5: Restore SUMMARY from current evidence only**

Set frontmatter to `status: complete`. Replace the blocked notice with exact commit IDs and actual command results. Record Linux/WSL-only scope, warmed cache/Rust 1.96/build-Python prerequisites, fixed launchers, two measured prebuilt slice times, isolated workspace result, archive tree IDs, zero-residue evidence, and both review dispositions. Do not restore old Shell-authority claims.

- [ ] **Step 6: Commit only the restored SUMMARY**

```bash
git add -- \
  .planning/phases/01-baseline-evidence-governance/01-03-SUMMARY.md
test "$(git diff --cached --name-only)" = \
  .planning/phases/01-baseline-evidence-governance/01-03-SUMMARY.md
git commit -m "docs(01-03): restore completion after supervisor gates"
test -z "$(git diff --cached --name-only)"
```

- [ ] **Step 7: Re-run the final delivered-HEAD archive gate without repository writes**

```bash
final_head=$(git rev-parse HEAD)
final_root=$(mktemp -d /var/tmp/kiana-supervisor-final-XXXXXX)
git archive "$final_head" | tar -x -C "$final_root"
test ! -e "$final_root/reference"
(
  cd "$final_root"
  env -u CARGO_BUILD_TARGET cargo build --target-dir "$final_root/target" \
    -p kiana-capability-governance-supervisor --locked --offline
  bash scripts/capability-governance-supervisor-smoke.sh
  bash scripts/capability-governance-smoke.sh schemas
  bash scripts/capability-governance-smoke.sh fixture-shapes
)
rm -rf "$final_root"
test "$(git rev-parse HEAD)" = "$final_head"
git status --short
```

Expected: final HEAD passes without a repository edit. Normal GSD routing may resume for `01-03`; starting `01-04` remains a later explicit workflow action.
