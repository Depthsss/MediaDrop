# Installer Worker Protocol

MediaDrop setup protocol version 1 has two deliberately separate channels.

## NSIS to broker

The normal-integrity setup creates
`%LOCALAPPDATA%\MediaDrop\InstallerSessions\<guid>\` and writes UTF-16LE
`config.ini`. The broker atomically replaces `status.ini`; NSIS atomically
replaces `command.ini`. Both files carry a positive, monotonically increasing
`sequence`. A stale sequence is ignored.

`status.ini` contains `protocol`, `sequence`, `state`, `heartbeat`, `progress`,
`phase`, `action`, result codes, reboot state, the MSI log path, and broker and
worker PIDs. Text is length-limited and control/INI delimiter characters are
removed. Commands are limited to `cancel`, `retry_files`, `continue_files`, and
`cancel_files`.

These files are a UI transport, not a privilege boundary. The elevated worker
never trusts them directly.

## Broker to elevated worker

The broker creates a remote-rejected named pipe whose DACL grants the
interactive user, Administrators, and SYSTEM. Frames are:

```text
u32 little-endian JSON byte length
JSON payload (maximum 65,536 bytes)
```

Every message carries `protocol: 1` and a monotonically increasing sequence.
Broker commands are `start_install`, `cancel`, `abort`, and a bounded
files-in-use response. Worker events are `hello`, `status`, and `complete`.

Before `start_install`, the broker validates all of the following:

- the 256-bit session secret using constant-time comparison;
- the session ID and protocol version;
- the claimed PID against `GetNamedPipeClientProcessId`;
- an elevated worker token;
- the broker and worker Windows session IDs;
- the worker image path against the broker's own executable path.

`ShellExecuteExW("runas")` is only the UAC request. A successful call or an
optional process handle is not proof that the worker started; the authenticated
pipe handshake is.

## Lifecycle guarantees

`cancel_pending` and `rolling_back` are non-terminal. The setup displays a
terminal cancellation only after Windows Installer returns and the broker
publishes `failed` with MSI code 1602. A broken parent or pipe sets the same
safe cancellation flag; no installer process is force-terminated.

Protocol changes are additive only within version 1. Any incompatible framing,
authentication, or command change requires a new protocol version and explicit
old/new compatibility handling.
