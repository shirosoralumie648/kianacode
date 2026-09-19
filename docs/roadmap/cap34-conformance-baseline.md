# CAP-34 capability conformance baseline (partial)

The domain now defines a bounded backend × profile × tool × scenario evidence matrix for Linux,
macOS, Windows and Container combinations. Rows are explicitly Verified, NotApplicable,
NotImplemented or Blocked. NotApplicable and NotImplemented rows require a concrete reason and
are never counted as verified. Each row binds catalog, credential and data epochs plus grant and
effect/resume fence observations and a platform backend disposition; grant drift, fence bypass or
Verified without an Implemented backend fails closed.

The current matrix is a source/CI closeout shape, not a full product acceptance. CAP-30 ranked
extension search/runtime, CAP-31 macOS, CAP-32 Windows and CAP-33 container/gVisor rows remain
partial or not implemented; CAP-34 therefore remains partial and does not turn skipped rows into
success. Live/physical, performance, cleanup-cost and cross-entry runtime evidence remain open.
