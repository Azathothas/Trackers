# Phase C backlog

Everything here needs a persistent process or a user at a keyboard. Decision 7.4
settles it: none of this is built in Phase A or Phase B. It is written in the
same entry format as the rest so the backlog is executable rather than a wish
list, and so the Phase A architecture can be checked against it: the core
library must not assume a process lifetime, and state must be addressable.

**Do not work on this file.** It exists to keep the ideas out of the code.

## The aria2 parity checklist

The operator's retired brief held an aria2 parity checklist, so it is written
out here. It was entirely unstarted when it was written and it is entirely
unstarted now. Nothing in it is Phase A or B work: every item needs a process
that outlives an invocation.

**The RPC surface is 36 methods.** `reference/aria2_rust/docs/comprehensive_gap_analysis.md:390`
carries the list under "RPC Method Coverage (36/36)", one row per C++ method
with the Rust handler that answers it: the `aria2.addUri` / `addTorrent` /
`addMetalink` family, `remove` and `forceRemove`, `pause` / `forcePause` /
`pauseAll` / `forcePauseAll` and their `unpause` counterparts,
`purgeDownloadResult` and `removeDownloadResult`, the `get*` readers
(`getUris`, `getFiles`, `getPeers`, `getServers`, `getOption`,
`getGlobalOption`, `getVersion`), the `tell*` readers (`tellStatus`,
`tellActive`, `tellWaiting`, `tellStopped`), `changeOption`,
`changeGlobalOption`, `changePosition`, and the rest. That file is the
checklist for [T-201](#t-201-json-rpc-and-xml-rpc-with-aria2-method-parity):
it is a method-by-method table rather than prose, so parity is countable rather
than argued. Its own status line is worth copying too. The method surface is
complete and external compatibility stays `PARTIAL` until a browser-extension
and original-client interoperability matrix is reproducibly green, which is the
difference between implementing 36 names and being a drop-in.

**What a real migrant actually missed is shorter than the method list.**
gosh-dl [Issue 11](https://github.com/goshitsarch-eng/gosh-dl/issues/11)
(CLOSED) is one user moving off aria2 RPC, and the two things they named were
**batch pause and resume** and **`.aria2` control files**, the second because
without it there is no resume from breakpoint. Those are
[T-202](#t-202-queue-management-across-invocations) and
[T-203](#t-203-session-save-and-restore), and the issue is the evidence that
those two outrank the other thirty-four methods for anyone actually migrating.
Build them first.

**The `.aria2` control file is a format, not just a feature.** It sits beside
the payload and holds the bitfield and the per-file progress, which is what
makes an aria2 download resumable after a kill. `bit-cli` has no state file at
all by decision, and [T-016](disk-io.md) is blocked for the same reason, so
this is where the daemonless decision and aria2 parity actually collide.
Whoever un-defers Phase C has to answer it deliberately rather than inherit it.

---

### T-200 Session daemon

Source:      decision 7.4
Category:    phase-c
Priority:    P0 within Phase C
Effort:      XL
Status:      deferred

Problem:     Every `bit-cli` command starts a session, does its work, and
             exits. A torrent cannot outlive an invocation, so there is no way
             to add one now and check on it later.
Relevance:   It is the foundation every other item here sits on.
Approach:    A foreground process that owns a long-lived `Session` and serves
             the verbs over a local socket. The core library already keeps
             configuration explicit and holds no global state, which is what
             makes this addable without a rewrite.
Acceptance:  `bit-cli daemon start` runs, `bit-cli add <SOURCE>` returns
             immediately, and `bit-cli status` reports the torrent from a
             second process.

### T-201 JSON-RPC and XML-RPC, with aria2 method parity

Source:      decision 7.4, and the aria2 parity checklist above
Category:    phase-c
Priority:    P1 within Phase C
Effort:      XL
Status:      deferred

Problem:     No RPC surface of any kind.
Relevance:   `aria2.addTorrent` with a `uris` array is the only documented web
             seed surface any existing tool has, so parity with it is what lets
             an existing deployment migrate.
Approach:    `--enable-rpc` and the `--rpc-*` option family, `aria2.*` method
             names, and the secret token scheme.
Acceptance:  An existing `aria2` RPC client drives a download end to end
             without modification.

### T-202 Queue management across invocations

Source:      decision 7.4
Category:    phase-c
Priority:    P1 within Phase C
Effort:      L
Status:      deferred

Problem:     `-j` limits parallelism inside one invocation. There is no queue
             that spans invocations and no way to reorder one.
Relevance:   `--max-concurrent-downloads` in `aria2` is a queue depth, not a
             parallelism cap, and a migrating script will expect that.
Approach:    Needs the daemon. `changePosition` is the reordering primitive.
Acceptance:  Three torrents added with a queue depth of one run in the order
             they were added, and `changePosition` moves the third to the
             front.

### T-203 Session save and restore

Source:      decision 7.4
Category:    phase-c
Priority:    P1 within Phase C
Effort:      L
Status:      deferred

Problem:     `--save-session`, `--force-save`, and `--auto-save-interval` have
             no equivalent. `librqbit`'s `SessionPersistenceConfig` is
             deliberately left off.
Relevance:   Restarting a box should not lose a queue.
Approach:    Needs the daemon. The format has to carry enough to reconstruct
             the queue, the file selections, and the per-torrent limits.
Acceptance:  A daemon restarted mid-download resumes every torrent at the
             progress it had.

### T-204 Persistent attached web seeds

Source:      the operator's brief, the deleted `src/webseed/state.rs`
Category:    phase-c
Priority:    P2 within Phase C
Effort:      M
Status:      deferred

Problem:     `kist` persisted attached web seeds keyed by info hash, so a
             source attached once stayed attached across restarts. That file
             was deleted during the crate split, because a stored record is a
             session concept.
Relevance:   This is the one Phase C item that touches the headline feature, so
             the boundary matters: in Phase A and B, web seeds attach per
             invocation through flags, and `bit-cli edit` is how a source is
             made permanent by writing it into the `.torrent`. Only a daemon
             needs a third option.
Approach:    Keyed by info hash, stored alongside the session state from T-203,
             with the same binding table schema `--web-seed-config` already
             uses so the two are interchangeable.
Acceptance:  A source attached through the daemon survives a restart and
             appears in `bit-cli webseed list` against the running session.

### T-205 Download result registry

Source:      decision 7.4
Category:    phase-c
Priority:    P3 within Phase C
Effort:      M
Status:      deferred

Problem:     No `--max-download-result`, `--download-result`, or
             `purgeDownloadResult`.
Relevance:   It is how an RPC client learns that something finished while it
             was not looking.
Approach:    Needs the daemon.
Acceptance:  A finished torrent appears in the result list and is purgeable.

### T-206 GID assignment

Source:      decision 7.4
Category:    phase-c
Priority:    P3 within Phase C
Effort:      S
Status:      deferred

Problem:     Torrents have a per-run index and no stable identifier.
Relevance:   Every `aria2` RPC method takes a GID.
Approach:    Needs the daemon. The info hash is the natural key and is not
             unique when the same torrent is added twice with different file
             selections, so a GID is a separate identifier.
Acceptance:  `bit-cli add` returns a GID that `bit-cli status <GID>` accepts.

### T-207 Session-attached verbs from the old TUI

Source:      decision 7.4, `docs/command-mapping.md`
Category:    phase-c
Priority:    P2 within Phase C
Effort:      M
Status:      deferred

Problem:     `add` to a queue, `pause`, `resume`, `remove`, and marking a
             torrent all need something to mutate.
Relevance:   Six of the old `CommandId` variants map here.
Approach:    Needs the daemon.
Acceptance:  Each verb works against a running daemon.

### T-208 status --follow against a live session

Source:      decision 7.4
Category:    phase-c
Priority:    P3 within Phase C
Effort:      M
Status:      deferred

Problem:     `bit-cli` can stream progress for a download it is running itself
             (`--jsonl`), and cannot report on a download another process is
             running.
Relevance:   The distinction matters and is easy to blur: a streaming mode a
             single foreground invocation produces itself is Phase A;
             following someone else's session is Phase C.
Approach:    One subcommand serving both a one-shot query and a stream, with
             the mode a value rather than a separate verb: snapshot, follow,
             set the interval, stop. That shape keeps `--follow` from becoming
             a second command that drifts from the first.
Acceptance:  `bit-cli status --follow` streams events from a daemon.

### T-209 Watch directories, RSS, cluster mode, and the control service

Source:      decision 7.4
Category:    phase-c
Priority:    P3 within Phase C
Effort:      XL
Status:      deferred

Problem:     Everything that needs a running process: RSS ingestion, watch
             directories, Docker and VPN integration, cluster mode, and the
             control service.
Relevance:   Recorded so they are not rediscovered as Phase B ideas.
Approach:    All need the daemon.
Acceptance:  Deferred as a group.

---

### T-243 A user interface, and which of the two kinds decision 7.4 permits

Source:      the operator, 2026-08-24. Corpus: `RESEARCH.md` entries 40 and 41
Category:    phase-c
Priority:    n/a
Effort:      L for the native shape, XL for the browser shape
Status:      deferred, and it is a **draft**: it needs an operator ruling
             before it is workable at all

**This entry is a draft in the Phase C backlog and nothing here is started.**
It is filed here rather than in a category file because half of what it
describes is Phase C by decision 7.4, and because the half that is not still
needs a ruling.

### The collision, stated so the operator rules on it with the conflict visible

[RULES.md](RULES.md) section 6, decision 7.4, is "no daemon and no RPC", with
no SQLite and no state file, and `bit-cli` must keep working with no config and
no state. Section 6 also says the corpus's daemon stack and control-plane
designs are for [T-200 to T-209](phase-c.md) only, and do not un-defer them.

**A browser UI reverses that decision by construction.** For a page to reach
`bit-cli`, `bit-cli` has to listen. That is [T-200](phase-c.md), the daemon,
and [T-201](phase-c.md), the RPC. A UI that shows a torrent list across
invocations needs [T-203](phase-c.md), session save and restore. So the
TypeScript option the operator raised is not a UI decision with a daemon
attached; it is the daemon decision, with a UI attached.

The operator reopened the `iroh` line of section 6 on 2026-08-24 and **did
not** reopen 7.4. This entry does not treat it as reopened.

### The distinction the brief did not draw, and it is most of the answer

**A native GUI does not collide with 7.4 at all.** A second binary linking
`bit-cli-core` and driving the same session the CLI drives: no listener, no
RPC, no state file, and `bit-cli` itself unchanged and still working with no
config. Everything 7.4 forbids is absent because there is no server. The UI is
the process.

So the question the operator is actually being asked is not "should there be a
UI" but "which kind", and only one of the two costs a settled decision.

### The recommendation, and the runner-up

**Best candidate: `egui`.** From `RESEARCH.md` entry 41, the 2026 survey of
fifty-four Rust GUI libraries: `egui` and `slint` are the two winners, and both
are named for input method and accessibility support rather than for looks.
`egui` wins here on three counts that are about this repository rather than
about the libraries:

- **Immediate mode matches what `bit-cli` already produces.** The CLI's report
  is a snapshot rebuilt every `--report-interval`, and `--jsonl` is a stream of
  those snapshots. An immediate-mode UI redraws from a snapshot, so the data
  path is what the terminal renderer already does with the same struct. A
  retained-mode UI wants a model with change notifications, which is state
  `bit-cli-core` does not keep.
- **It is one crate and no build step.** No code generator, no separate markup
  language, no Node in the release path. This repository builds three targets
  on every push, and its CI is twenty-one jobs.
- **The survey's one caveat against it is CJK font setup for input methods**,
  which is a font to ship and not a design problem.

**Runner-up: `slint`, and why it lost.** It ties `egui` on accessibility and
input methods and its markup language is genuinely better for a table-heavy
interface, which a torrent list is. It loses on the build: its markup files are
compiled by a build script, so the release path gains a code generation step,
and the survey measured its workspace at 4.2 GB against `egui`'s 1.9 GB. Both
are tolerable and neither is free; the tie breaker is that `egui` adds nothing
to the build at all.

**The TypeScript option, evaluated honestly rather than dismissed.** It is
genuinely better on three axes and worse on four.

Better: less Rust to write and maintain; a table with sorting, filtering and
virtual scrolling is a solved problem in a browser and is weeks of work in any
Rust GUI; and it is reachable from a phone, which no native binary is.

Worse: it needs the daemon, which is the whole of the collision above. It needs
a Node toolchain in the release path, which this repository does not have and
which is a second supply chain to watch after [T-199](cli-surface.md) put the
first one under an automated bump. It needs an HTTP API surface that becomes a
compatibility obligation the moment anybody scripts against it. And it needs a
browser, which is a dependency the CLI does not have.

**The worked example is on this exact base.** `rustTorrent`
(`RESEARCH.md` entry 40) is `rqbit` with a React and TypeScript web UI, a full
HTTP API, qBittorrent-compatible endpoints for the media automation tools, RSS
automation, a Docker image and a Tauri desktop wrapper. It is the finished
version of the browser answer, built on the engine `bit-cli` vendors, and it is
the honest price list.

### What the operator is being asked to rule on

1. Is a UI wanted at all?
2. If yes: native, which 7.4 permits, or browser, which reverses it?
3. If browser: is 7.4 reopened, and are [T-200](phase-c.md),
   [T-201](phase-c.md) and [T-203](phase-c.md) un-deferred with it?

Recommended: yes to 1, native to 2, and 3 does not arise.

### What this draft owes before it is workable

Nothing is startable from this entry as it stands, and that is deliberate.
Until the ruling, it has no acceptance command, because what would be measured
depends on which shape is chosen. When a shape is chosen the acceptance is the
same either way and it is worth writing down now: **the UI must not be able to
do anything the CLI cannot**, and the check is that every action it offers maps
to a documented `man/bit-cli.json` command, so the UI is a second face on one
surface rather than a second surface.
