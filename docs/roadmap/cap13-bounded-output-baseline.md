# CAP-13 bounded output, redaction and artifact baseline

CAP-13 introduces one `ExecutionOutputBudget` for collection, preview, persistence and observed
limits. The daemon output module owns bounded byte/line capture, bounded pipe drain, UTF-8-lossy
projection, ANSI/OSC filtering and redaction before display or persistence. Metadata reports both
captured/observed counts and the exact budget/error boundary; quota and drain failures remain
visible instead of becoming an empty successful output.

Shell and long-running process paths use the shared redaction/preview boundary. Long output is
stored through the existing scope/data-epoch-bound `ExecutionOutputRef`; persistence is size
checked, preview is separately bounded, cursor reads recheck owner, digest, expiry and data epoch,
and invalid/stale artifacts fail closed. This step does not claim an external artifact service or
cross-process durable retention policy.

GitHub Actions runs budget contract fixtures, the focused daemon output-cap fixture, CAP-13 source
guards and workspace compilation. No local runtime tests or smoke commands were run.
