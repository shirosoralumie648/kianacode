# P4-J7-20 protected provider replay baseline

This source slice adds an explicit scope contract and a bounded in-process store for private
reasoning/signature material. References bind connection, protocol, model, route, prompt/tool/data
digests, source call and expiry; receipts and EventLog can carry only the reference metadata.
Deletion and expiry fail closed. The provider compiler validates a reference envelope and refuses
to dispatch when a protected artifact adapter is unavailable; it never guesses at private bytes.

GitHub Actions owns all fixtures. No local test, build, check, clippy or smoke command was run.
The proof ceiling is `source` plus CI wiring: durable encrypted artifact persistence, provider
specific signature decoding, cross-process resume and live provider behavior remain open follow-up
work. No private reasoning bytes are written to the repository, EventLog or receipt.
