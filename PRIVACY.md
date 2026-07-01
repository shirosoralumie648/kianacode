# Privacy Notice

## Default Behavior

The current workspace does not include automatic product telemetry by default. Kiana can still send data out when a user explicitly configures and invokes networked features such as model providers, web fetch/search, HTTP/SSE/WebSocket MCP servers, remote sessions, bridge workers, or live smoke tests.

## Local Data

Kiana may store local configuration, permissions, hooks, plugins, tasks, and session history under the configured Kiana home directory. By default this is `~/.kiana`, with the main config at `~/.kiana/config.toml`.

Potentially sensitive local data includes:

- API keys and provider settings.
- Prompt, transcript, tool-call, and session metadata.
- MCP server headers and environment-expanded values.
- Plugin, skill, and hook definitions.
- Remote session IDs, bridge credentials, and token refresh command output.

## Networked Data

When enabled, Kiana may transmit prompts, tool results, files selected by the user, repository metadata, remote session events, MCP requests, and provider-specific parameters to configured endpoints. The endpoint operator's policies apply to that data.

## Logs and Support Bundles

Before sharing logs or support bundles, redact tokens, API keys, MCP headers, file paths that reveal private projects, prompts, transcripts, and customer data. A commercial release should include a first-class redaction and support-bundle workflow before broad distribution.

## Deletion

Remove local state with:

```bash
rm -rf ~/.kiana
```

Also remove installed binaries, shell completion files, shell profile PATH entries, and any external provider or remote-service data according to those services' policies.
