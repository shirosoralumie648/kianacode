# CI-09 OAuth / workload identity lifecycle 基线

> 快照日期：2026-09-18。运行时验收由 GitHub Actions 执行；本地不运行测试或 smoke。

## Lifecycle contract

domain `OAuthAuthorizationRequest` 只保留 flow/client/endpoint/redirect/scope、state digest、
S256 code challenge、时间窗和 request digest；`OAuthCallback` 只携带一次性 code/state/redirect；
`OAuthTokenMetadata` 只携带 provider-account、User/Workload subject、generation、scope、expiry、
access/refresh digest 和状态。未知字段、未排序/重复 scope、过期 flow、非法 endpoint 或 digest
均 fail-closed，raw access/refresh token 没有 domain 字段。

provider `OAuthManager` 在进程内保存 pending state/verifier，并生成 RFC 7636 S256 PKCE URL；
callback 必须匹配 flow/state/redirect 且只能消费一次。token response 使用 strict bounded JSON
decoder（16 KiB response、8 KiB token、Bearer、positive bounded expiry、required scopes），不
接受未知字段、超限、坏 token 或 scope 不足。

已有 token 临近 expiry 时进入 single-flight refresh：一个 leader 使用 `Notify` 执行 refresh，
并发 caller 复用结果；短暂错误保留仍有效的 access token 并设置 cooldown，永久错误进入
`ReauthRequired`，provider 标记的撤销进入 `Revoked`。成功 refresh、显式 rotation 和 revoke
都用 generation CAS；旧 refresh 完成时不能覆盖新 generation，revoke 后状态/generation
立即 fence 后续使用。

可选 token file 是 provider-only secret adapter：严格 `kiana.oauth-token-file.v1` envelope，
create-new temporary file → write/sync → chmod 0600 → atomic rename → parent sync，目录新建为
0700，目标 symlink/宽权限拒绝。文件仅由 provider 读写，projection/diagnostics 只取 metadata。

## CI-only evidence

`.github/workflows/ci09-oauth-lifecycle.yml` 执行 domain contract、provider PKCE/callback/
single-flight/CAS/error/file fixtures 与 core source guard，并编译 workspace test targets；不
联系真实 IdP、OAuth tenant 或 provider。工作流由本提交触发，本地只做格式、静态编译和 diff
检查，不等待 CI。

## Limitations

- OAuthManager 尚未接入真实 HTTP token endpoint、ProviderGateway credential adapter 或跨进程
  durable identity projector；exchange/refresh 通过受控 closure 注入，fake success 不等于 live。
- token file 仍在进程/磁盘中保存 raw token；0600/atomic 是文件边界证据，不是 HSM、物理内存
  擦除或外部 IdP 撤销证明。
- 真实 workload attestation、PKCE browser callback listener、scope policy、OAuth account
  discovery、rotation/revoke EventLog receipt 和跨设备恢复留 CI-10/11、PD/SC/ER；旧
  `kiana-services` OAuth facade 保持兼容，不作为新的 ControlPlane 执行路径。

## 2026-10-04 new-grant generation fencing

Installing or authorizing a new grant now increments the current generation instead of resetting
it to `1`. Pending authorization records capture the observed generation; callback consumption
and exchange completion must still match that generation. Token persistence and publication
share the same manager lock, so an older refresh or authorization response cannot overwrite a
newer grant or a revoked token file. Generation overflow preserves the current grant.

The CI fixtures hold an old refresh across each of four grant entrypoints and then release
success, transient, permanent and revoked outcomes. All 16 races require a generation conflict
and unchanged replacement metadata/file bytes. Additional fixtures cover revoke during a
pending callback or asynchronous exchange, file reload and generation overflow. Domain fixtures
assert both sides of the refresh skew boundary.

Run `37161948995` at `6abe23e9` passed the pre-repair focused lane after correcting the workload
enum source guard. Changed-source validation is submitted to GitHub CI after this commit;
no local test/build/check/clippy/smoke or validation script runs. The card remains partial/source:
Gateway/IdP integration, cross-process token-file CAS and product-chain recovery remain open.

The changed-source run `37162423852` passed the 16 held-refresh races, but its revoke/reload
fixture exposed a token-file inconsistency: revocation removes refresh material while keeping
its digest in memory, and the old codec reconstructed metadata only from remaining material.
The file envelope now stores an optional refresh digest, preserving revoked/reauth metadata
across reload. A present token must match that digest; an active token cannot claim a refresh
digest without material. Old active v1 files still derive the digest from their token. Historical
files that already discarded the digest cannot recover it. Additional CI fixtures cover all
three fencing transitions, zero refresh after reload and malformed/missing material binding.

## 2026-10-17 explicit generation CAS and scope-insufficient fencing

Two named lifecycle contracts from the §29.5 target were still open and are now covered at the
provider boundary:

- `OAuthManager::invalidate_if_current(observed_generation)` implements the documented
  compare-and-swap revocation. An empty manager reports `oauth_token_missing`; a stale generation
  reports `oauth_generation_conflict` and leaves the newer grant (metadata, access material and
  token file) byte-for-byte unchanged; the current generation is revoked with a `revoke()` that
  advances the fence before persistence and notification, so a concurrent in-flight refresh of
  that generation cannot reinstall it. This is the sibling of `refresh(observed_generation)`
  (`rotate`), not a replacement for it.
- A refresh response whose scope is insufficient is no longer reported while the old token stays
  `Active` and repeatedly refreshable. `finish_refresh` now treats `OAuthError::ScopeInsufficient`
  as permanent for that grant: it calls `metadata.reauth()` (advance generation, drop refresh
  material, keep the refresh provenance digest, status `ReauthRequired`) and returns
  `oauth_scope_insufficient`. Subsequent `access_token` calls fail with `oauth_reauth_required`
  and never issue another refresh.

Fixtures: `invalidate_if_current_is_generation_cas_and_denies_stale_callers` (deny
missing/stale, accept current, then fenced-after-revoke with zero refresh) and
`insufficient_scope_refresh_fences_to_reauth_without_replacement` (deny-first scope downgrade,
`ReauthRequired` metadata, zero further refresh, explicit reauthorization). The core source guard
`ci09_oauth_guard` gained the `invalidate_if_current` marker. All three run under the existing
`.github/workflows/ci09-oauth-lifecycle.yml` (provider via `--lib oauth::tests`); no new target,
manifest or execution path was added. No local test/build is run in this session (CI-only);
`feature_status=implemented`, `proof_level=source`.

Still open (unchanged): real IdP/token-endpoint HTTP, `ProviderGateway` credential-backend
integration, cross-process durable identity/revocation, the separately stored external account
subject, workload attestation, scope policy, OAuth receipt/audit and external/live/physical proof
remain CI-10/11 and PD/SC/ER follow-ups.
