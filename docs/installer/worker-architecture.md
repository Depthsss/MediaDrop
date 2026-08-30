# MediaDrop Installer Worker Architecture

## Decision

MediaDrop's branded Windows installer uses a normal-integrity NSIS UI and one
purpose-built x64 worker executable:

```text
asInvoker NSIS UI
  -> normal process launch
x64 worker --broker
  -> ShellExecuteExW("runas") only as the UAC boundary
x64 worker --elevated-worker
  -> authenticated named pipe
  -> Windows Installer API
  -> MediaDrop MSI
```

`ShellExecuteExW` success is not installation success. The elevated worker is
accepted only after a versioned handshake whose PID, session, elevation token,
image path, and session secret all match. A returned shell process handle is
optional supervision data.

## Trust boundaries

- NSIS and the broker run as the interactive user. Browser detection, HKCU,
  clipboard, extension setup, and post-install launches stay here.
- The elevated worker accepts only the MSI identity compiled into its binary:
  SHA-256, byte length, ProductName, Manufacturer, ProductVersion, UpgradeCode,
  and x64 package template.
- NSIS communicates with the broker through bounded, sequence-numbered INI
  files in the user's installer-session directory. Those files are not a
  privilege boundary.
- Broker and elevated worker use a remote-rejected named pipe restricted to the
  interactive user, Administrators, and SYSTEM. Privileged commands cross only
  this authenticated channel.
- Neither the wrapper nor worker creates an ARP entry, service, scheduled task,
  or updater. The existing Tauri MSI remains the only installed product.

## Lifecycle

The worker reports monotonically revisioned states:

```text
extracting -> starting_broker -> awaiting_elevation -> handshaking
-> verifying_payload -> preparing_installer -> installing
-> files_in_use | cancel_pending -> rolling_back
-> succeeded | failed | elevation_cancelled
```

MSI progress comes from `MsiSetExternalUIRecord`. Cancellation sets an atomic
flag and the external UI callback returns `IDCANCEL`; no installer or worker
process is force-terminated. Completion is reported only after
`MsiInstallProductW` returns.

## Release boundary

Build and local integration checks do not make the installer publishable. A
clean Windows 10/11 VM matrix with real UAC, install, cancel, upgrade, updater,
file-in-use, standard-user, and Unicode-profile scenarios remains a release
gate.
