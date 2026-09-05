# shell.md

Traps in passing text and reading results between shells.

Every entry is a defect somebody actually hit, and most of them fail silently,
which is why they are worth a document rather than a comment. The shape of
almost all of them is the same: **a payload crosses a boundary and loses its
quoting, or a result crosses a boundary and loses its meaning.**

⭐ **The gates themselves do not depend on any of this.** RULES 15.5 puts every
check in Python for exactly that reason: a `.sh` a gate needs is a platform
requirement in disguise. This page is for the sessions and the operators who
still have to drive a shell.

---

## 1. A prose payload goes through a file, not through a shell

⛔ **Write the text to a file with a file-writing tool, then pass the path.**
This covers a commit message, a document, a script, a JSON body, anything
multi-line, and anything containing an apostrophe, a backtick, a dollar sign or
a backslash.

The reason it is a file rather than better quoting is that quoting is not
sufficient. A payload handed to a shell as an inline string inside a quoted
heredoc is not reliably literal, and backticks in the prose have been executed
from inside one.

⚠ **Measured in this repository on 2026-08-31**, on one Windows 11 host through
this harness's Bash tool: a quoted heredoc `<<'PY'` carrying Python source lost
one backslash from every `\\n`, twice in one session, so a comment written as
`newline="\\n"` arrived as a real line break and the file no longer parsed. The
failure was loud both times because Python refused the file. ⚠ **The same
substitution inside a regular expression or a document would not have been
loud**, and that is the case this rule exists for.

⭐ **Verify it in the environment you are in rather than assuming either way:**

```bash
printf 'literal: C:\\Users and regex \\d+\n'
```

If the output shows one backslash where two were written, every literal double
backslash has to go through a file-writing tool.

Two related failures with the same cause:

- A PowerShell here-string written inside a `bash` command is parsed by bash
  first, so it ends at the first apostrophe in the text.
- `python -c` and `python - <<'PY'` are fine for code with no apostrophes and
  no backslashes. A Windows path in a Python string literal has both.

⭐ Where a payload has to cross a shell at all, **base64 is the one encoding no
shell interprets.**

---

## 2. An exit code is read from the process that produced it, unpiped

⛔ Piping a check into anything reports the **pipeline's** status, not the
check's, so a guard that failed reads as green.

```bash
python3 scripts/check-gate.py
```

Not into `grep`, not into `Select-String`, not into `tee`. Run it, read the
status, then look at the output separately.

⚠ In PowerShell, `-ErrorAction SilentlyContinue` suppresses the error *output*
while the failure still sets a failing status. To make one genuinely non-fatal,
promote it and swallow it:

```powershell
try { Some-Cmdlet -ErrorAction Stop } catch { $null = $_ }
```

---

## 3. stdout and stderr are different streams

⛔ **Anything reading a value reads stdout alone and checks the exit code.**
Merging is correct only where the wanted thing is on either stream, and then it
is a decision with a comment attached.

⚠ `git rev-parse --abbrev-ref HEAD` in a repository with no commits prints
`HEAD` to stdout **and** a fatal to stderr, exiting 128. `java -version` prints
its version to stderr alone. Both directions are real.

---

## 4. Line endings

A carriage return in a file `.gitattributes` says is LF is **invisible to git
and visible to everything else.** The index is normalised either way, so
`git diff` shows nothing and a review cannot see it.

```bash
git ls-files --eol
```

The `i/` column decides what a commit contains. Every text file this project
owns should read `i/lf`. ⚠ One tracked file is deliberately `i/crlf`:
`references/XIU2__TrackersListCollection/tree/index.html`, because
[`../../.gitattributes`](../../.gitattributes) marks the whole corpus `-text` so
the captured bytes stay the captured bytes.

⛔ **Python's text mode translates on write.** `open(path, "w",
encoding="utf-8")` writes CRLF on Windows, so the same instrument produces
different bytes on a contributor's machine and on a runner. Two writers in this
repository did exactly that until 2026-08-31. Every text write passes
`newline="\n"`, and RULES 15.5 is the rule.

⚠ **`.ps1` keeps CRLF**, and that is not a preference: Windows PowerShell 5.1
mis-parses a here-string whose terminator arrives with a bare LF.

---

## 5. A control byte goes in a file as an escape, never as itself

⛔ A literal control byte makes the file invisible to both review tools at
once, and the runtime value is identical either way, so only reviewability is
ever at stake. Write the escape.
[`../../scripts/check-control-bytes.py`](../../scripts/check-control-bytes.py)
refuses one over every tracked text file.

---

## 6. Windows

- ⛔ **`python3` may be a stub that is on `PATH` and does not run.** Measured
  on one Windows 11 host on 2026-08-31: `python3 --version` printed a Microsoft
  Store advertisement and exited **49**, while `python` was a working 3.13.15.
  ⭐ **Presence is not capability: probe by running the tool, not by finding
  it.** Every command in this repository's documents is written `python3`
  because that is what a POSIX host and the CI runners have; on a host where it
  resolves to the stub, substitute `python`.
- ⛔ **Python on Windows cannot print this repository's own markers by
  default.** stdout is a legacy code page with no ⛔, no ⭐ and no ⚠, and the
  failure is at print time, so it passes every test that captures output and
  fails the moment a person runs it at a console. Every check here calls
  `sys.stdout.reconfigure(encoding="utf-8")` through
  [`../../scripts/_scope.py`](../../scripts/_scope.py).
- ⭐ **Git Bash rewrites arguments that look like POSIX paths.** Anything with
  a leading slash becomes a Windows path before the target process sees it, and
  when the target is not a Windows program the rewrite is silent corruption.
  Two variables turn it off and they cover different things:

  ```bash
  MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL='*' podman run --rm alpine ls /etc
  ```

- ⛔ **The reserved device names are `CON`, `PRN`, `AUX`, `NUL`, `COM1` to
  `COM9` and `LPT1` to `LPT9`**, in any case and with any extension. A
  `2>/dev/null` under a shell that does not map it creates a real file called
  `nul`, which git then tracks, which breaks `git stash`, and which cannot be
  removed by `rm` or by Python. They are in
  [`../../.gitignore`](../../.gitignore) before any of it happens.
- ⚠ **An ephemeral port free for one protocol is not free for the other.**
  Windows keeps *separate* port exclusion ranges per protocol inside the
  dynamic range, and a fixture that binds UDP to port 0 and then binds TCP to
  whatever came back fails intermittently with `WinError 10013`. Measured
  2026-09-05 on this repository's Windows 11 host: **25 excluded TCP ranges and
  23 excluded UDP ranges**, different sets, inside 49152-65535.

  ```bash
  netsh int ipv4 show excludedportrange protocol=tcp
  ```

  ⭐ **Retrying `bind(0)` does not escape it.** Ephemeral ports are handed out
  roughly sequentially and the excluded blocks are about 100 ports wide, so
  consecutive retries walk through a block rather than away from it: twenty in a
  row failed. Decorrelate the retry by picking a random port in the range.
  `tests/fake_dns.py` carries the working version.

- ⚠ **`/tmp` is not one directory.** Git Bash resolves it inside the msys root
  and a native Windows Python resolves it somewhere else or not at all, so a
  file written by one and read by the other is not found. RULES 15.5 forbids it
  in a code path; use `tempfile`, or a path both agree on.
- ⚠ **`curl` in Windows PowerShell 5.1 is an alias for a cmdlet** that takes
  entirely different arguments, so `curl -sSL -o FILE URL` there is not a
  download and fails in a way that does not mention curl. Use `curl.exe` by
  name, or `Invoke-WebRequest`.
- ⚠ **`sort` in a native PowerShell session resolves to a cmdlet alias** that
  compares case-insensitively. Over `b A a B a` it returns two values where
  coreutils returns four. A missing tool fails loudly; an aliased one succeeds
  and returns a different answer.
- ⚠ **A machine-wide install is not under the user's home.** Checking only the
  user's directory reports a tool absent on a machine that has it.

---

## 7. PowerShell

- ⛔ **`[int]` on a double rounds.** `[int](2.65)` is 3. Use `[math]::Floor`.
- ⛔ **`-match` is case-insensitive**, so a pattern for `FAILED` matches
  `0 failed` in a summary line. Use `-cmatch` where case is the signal.
- ⚠ **`Get-Command` finds cmdlets, functions and aliases too.** Filter to
  `Application` when an executable is meant.
- ⚠ **Read a child process's streams before waiting on it.** Waiting first
  deadlocks any child that fills the pipe buffer.
- ⚠ **`ConvertTo-Json` defaults to depth 2** and renders anything deeper as a
  type name. Pass `-Depth`.
- ⛔ **A `.ps1` with any non-ASCII byte needs a UTF-8 byte-order mark** if
  Windows PowerShell 5.1 has to run it, because 5.1 decodes a BOM-less file as
  the system code page. The file then works on the machine it was written on
  and breaks on the one it was written for.

---

## 8. A long-running command needs a hard time limit

⛔ Several tools block for as long as they are allowed to. A script that shells
out to unknown tools without a limit is a script that hangs, and one that hangs
is one nobody runs twice.

```bash
timeout 6 some-tool --version
```

⚠ Exit 124 is a coreutils timeout and 137 is a kill. **Both mean "it never
answered", which is a different fact from "it is not installed"** and belongs in
a different field. RULES 1.4 is the four-category rule that follows from it.

---

## 9. Waiting inside a session

⚠ **Do not end a turn to wait for something.** The conversation idles, the
harness times out, and the session dies with state half-changed.

⛔ **And do not substitute the harness's own scheduler or wake-up tool.** Those
end the turn by design: they work by giving control back and being re-invoked.
Using one to hold is the thing this section forbids under a different name.

⭐ **Wait on the work, not on the clock.** Block on the thing that finishes: a
process, a run, a file appearing. Where a run is genuinely remote, poll it with
a bounded loop inside the turn.
