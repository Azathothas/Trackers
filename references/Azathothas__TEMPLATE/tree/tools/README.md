# tools

Helpers that genuinely need compiling.

⭐ **This directory is empty, and that is the correct current state.** Nothing
in this template has needed a compiled helper yet. Saying so is more useful
than shipping a binary to prove the directory works.

⛔ The bootstrap **deletes this directory** unless the plan names something that
belongs here.

---

## Before adding one

A compiled tool costs more than a script, forever: a toolchain to build it, a
build step in the gate, a binary per platform, and a rebuild whenever the
language moves. ⭐ **The bar is that a script genuinely cannot do the job**, not
that a compiled one would be nicer.

Three questions, in order.

**1. Can a portable script do it?** Most things a project needs are file
reading, text matching, process running and JSON emitting, and a POSIX shell or
PowerShell script does all of them with no build step. The probe in
[`../scripts/doctor/`](../scripts/doctor/) walks a filesystem, spawns eighty
processes with timeouts, and emits a schema, in two script files.

**2. Does a tool that already exists do it?** ⚠ Reaching for a general tool
where a purpose-built one exists produces answers that are plausible and wrong,
and writing a new one where a good one exists is the same mistake with more
steps.

**3. Is the reason one of these?** These are the reasons that actually justify
compiling:

- **Speed at a scale a script cannot reach.** Not "a script felt slow": a
  measured number, on the real input size, with the conditions beside it.
- **A library binding with no command-line equivalent.** Parsing a binary
  format, speaking a protocol, using a system API.
- **A single distributable binary** for a machine where installing a runtime is
  not possible.

⛔ If the reason is not on that list, the answer is a script.

---

## If it is genuinely needed

**Prefer Go or Rust**, whichever is easier, faster to build, and more
extensible for the task. Both produce a single static binary with no runtime to
install, which is the property that made compiling worth it.

⛔ **If neither can do it, double-check that conclusion before proposing a third
language.** Then bring it to the operator with: what you need, why Go and Rust
cannot do it, what has to be installed, from where, and what it costs to keep
installed. That is a decision, not a detail.

The rules that apply:

- ⭐ **The same check contract as a script**, from
  [`../scripts/README.md`](../scripts/README.md): a header saying what defect it
  catches, exit 0 pass, 1 fail, 2 could not run, a json flag, no dependence on
  the directory it runs from.
- **The source is committed.** A binary whose source is not in the tree is a
  binary nobody can audit, fix, or rebuild for another platform.
- ⛔ **A committed binary is a decision, not a convenience.** It has to be
  rebuildable from the committed source by a documented command, and the
  document has to say which platform each committed binary is for. A binary
  nobody can reproduce is worse than no binary.
- **The build is part of the gate**, or the binary drifts from its source
  silently.
- ⚠ **A checked-in binary is invisible to review.** A diff says only that the
  files differ. Whatever it does, the source is what gets read, so the source
  is what has to be readable.
