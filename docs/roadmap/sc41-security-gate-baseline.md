# SC-41 security and release gate baseline (partial)

SC-41 adds a CI-only structural gate for security workflows and release scripts. It requires
read-only workflow contents permission, fail-closed shell modes, no ignored failures, and linked
secret/SBOM/signature/checksum/license/compliance boundaries across SC-00, SC-18/19, DEP-39 and
DEP-41. The existing security baseline fixture remains the source/asset/constitution boundary.

The gate also indexes the typed `SupplyChainReleaseEvidence` and `ReleaseUatEvidence` handoffs:
release gate readiness is not a publish receipt, and UAT matrix evidence is not a durable/live
deployment. These references preserve approval, rollback, receipt, Unknown and limitation fields.

This gate does not certify authentication, SecretStore enforcement, complete redaction, provider
accounts, signed production artifacts, live deployment, physical effects or regulatory compliance.
It is partial/source evidence only; SC-42 recovery/retention rehearsal and SC-43 status/module-map
review remain subsequent steps.
