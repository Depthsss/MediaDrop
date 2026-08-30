# Legacy Setup Crash Diagnostics

The preserved local crash candidate is intentionally ignored by Git and must
not be published. Its frozen evidence is:

- source revision: `16b6f7832bd925a2e016502700c1e3cafe78a074`
- source tree was dirty at capture time
- setup source SHA-256: `FE02453D0441952A4C2934727FB045D80B6FCAEF2AED88595FD538E651BF79EA`
- executable size: `282999251` bytes
- executable SHA-256: `F6FA37FB97D576E5B08F9D71EDE07DC28E08A4813C4785D46D7EB464F35A8D57`
- NSIS: `3.11`
- WER module/exception/offset: `SHELL32.dll`, `0xc0000005`, `0x0010bd1d`
- callback exception: `0xc000041d`
- WER bucket: `56b8d5f4f1ee7e97b48b83c00fd42ed2`

Enable full dumps for the exact filename with
`Enable-LegacySetupDumps.ps1`, reproduce once, and immediately disable the
override with `Disable-LegacySetupDumps.ps1`.

Basic WinDbg sequence:

```text
windbgx -z <full-dump.dmp>
.symfix
.reload
!analyze -v
~* k
lmvm shell32
```

Record the failing thread, exception context, native stack, loaded module
versions, and whether heap corruption predates the SHELL32 fault. Do not state
that a timer, COM mode, or `ShellExecuteExW` re-entry was the exact cause until
the dump proves it. The new worker architecture is independently justified by
the product's elevation, progress, cancellation, and user-context requirements.

Full dumps can contain paths, command lines, and private process memory. Keep
them local, redact before sharing, and never commit the dump or crash EXE.
