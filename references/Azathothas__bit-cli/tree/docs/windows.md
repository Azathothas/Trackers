# On Windows

`bit-cli` is developed on Windows and the platform-specific behaviour is tested
rather than assumed. The entries behind this are in
[`TODO/windows.md`](../TODO/windows.md).

## Names the filesystem refuses

Every path in a torrent is planned before anything is opened, so a name NTFS
will not accept never reaches an open call. Reserved device names, trailing
dots and spaces, characters NTFS refuses, and paths past the legacy length
limit are all renamed rather than failing the download.

Two names that differ only in case collide on NTFS and both are kept, under
different final names.

Every rename is reported in `--json` under `renames`, with the reason. A
component that was dropped entirely, such as an empty path element, is reported
as `DroppedComponent`.

## A downloaded executable runs while the seeder is serving it

A payload file opens when it is first touched, a read opens for reading only
and does not create it, and a write opens for writing and does. So `bit-cli
seed` holds no handle that stops another process running the file it is
serving.

## Redirecting JSON output on Windows

`bit-cli` writes UTF-8 with no BOM to stdout whatever the console code page is.
Getting those bytes into a file is the caller's half and it is two decisions,
neither of which defaults to UTF-8:

| setting | what it decides |
| --- | --- |
| `[Console]::OutputEncoding` | how the host decodes what a program wrote |
| `$OutputEncoding` | how the host encodes what it sends into one |

A name carrying a character outside the console code page is corrupted on the
way through a pipeline, and the JSON still parses, so nothing says so.

```bash
pwsh -NoProfile -File scripts/check-redirect.ps1
```

That measures every documented form under both hosts and reports which give the
bytes back. It judges nothing: what it measures is a property of the host.
