# Persistent Codex stderr

Persistent app-server stderr is piped into a detached mode of the Workshop
executable, dispatched before instance locking or GUI initialization. The sink
has its own process group and continues while Codex holds its stdin pipe, so
closing Workshop does not remove the server's stderr reader. EOF ends the sink.

The sink redacts complete lines using the existing OAuth and diagnostics
redactors before writing. Lines larger than 64 KiB are discarded in full, with
a fixed omission marker; arbitrary chunks are never logged as partial secrets.
The file is owner-only on Unix and resets before exceeding 1 MiB. This is a
bounded diagnostic tail, not an archival log. It contains only the redactors'
output, not a parallel raw stream. Existing running servers retain their old
stderr destination until restarted; this code does not kill them automatically.

Qualification requires the packaged executable to enter sink mode, continued
Codex execution after UI exit, reconnect diagnostics, and secret-canary checks.
The standalone sink test covers bounded storage, oversized lines and redaction
ordering; it does not qualify packaged process lifecycle or the redactors'
coverage of every possible credential format.
