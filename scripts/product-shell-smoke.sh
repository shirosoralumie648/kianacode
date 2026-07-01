#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

cargo test -p kiana-screens --locked --offline render_
cargo test -p kiana-screens --locked --offline history
cargo test -p kiana-screens --locked --offline settings
cargo test -p kiana-entrypoints --locked --offline prompt_history
cargo test -p kiana-entrypoints --locked --offline load_settings_action_populates_readiness_hub
cargo test -p kiana-entrypoints --locked --offline direct_connect_app_contract
cargo test -p kiana-entrypoints --locked --offline tui::tests::

echo "product shell smoke passed"
