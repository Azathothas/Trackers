# TODO

Every entry, one line each, sorted by id. The entry itself lives in the
`TODO/<category>.md` the row links to, and it closes there with its own
acceptance command, actually run, with the output recorded.

**What to work on next is not here.** [PROGRESS.md](PROGRESS.md)'s "Start here
next session" is the work order and is the only place that carries one. This
file carries the list, the definitions, the counts, and the argument behind the
current ordering.

[RULES.md](RULES.md) is how this repository is worked on, including the only
sanctioned way to commit and push, what a blocked entry does instead of
closing, and why `phase-c.md` is written and never worked on.

`scripts/check-todo.ps1` checks this file against the entries: a status that
disagrees, a row with no entry, an entry with no row, a count that does not add
up, a `T-NNN` naming nothing, a dead link, and a cited path or line that does
not resolve.

```bash
pwsh -NoProfile -File scripts/check-todo.ps1
```

Superseded ordering arguments, the triage the first hundred entries came from,
and the record of what each closing measured are in
`reference/HISTORY/INDEX-history.md`. They were cut on 2026-08-24 because a
future session cannot act on them.

## Priority

- **P0** breaks correctness, loses data, or takes the process down.
- **P1** a documented capability does not work, or a flag does nothing.
- **P2** worth doing, nothing is wrong without it.
- **P3** worth recording so it is not rediscovered.

## Effort

S is under a day, M is a few days, L is a week, XL is longer.

## Entries

| ID | Priority | Category | Status | Item |
| --- | --- | --- | --- | --- |
| [T-001](webseed.md) | P0 | webseed | **done** | Measure the loopback bridge against a raw curl ceiling |
| [T-002](webseed.md) | P1 | webseed | **done** | Measure Candidate A-prime, the in-process virtual peer |
| [T-003](webseed.md) | P1 | webseed | **done** | The piece picker cannot be told to prefer HTTP |
| [T-004](webseed.md) | P2 | webseed | **done** | BEP 17 style is not auto-detected, only declared |
| [T-005](webseed.md) | P2 | webseed | **done** | A source restricted mid-run cannot be re-scoped |
| [T-006](webseed.md) | P1 | webseed | **done** | Prove the failure matrix against a real mirror |
| [T-007](webseed.md) | P2 | webseed | done | A stalling source takes 24 seconds to give up |
| [T-008](webseed.md) | P3 | webseed | **done** | A duplicate block request is fetched twice *(premise no longer reproduces: the counters are equal)* |
| [T-009](webseed.md) | P1 | webseed | **done** | A source cannot be attached over more than one connection |
| [T-010](disk-io.md) | P1 | disk-io | **done** | pwrite takes a read lock where it needs a write lock |
| [T-011](disk-io.md) | P1 | disk-io | **done** | No file handle pool, so long runs exhaust descriptors |
| [T-012](disk-io.md) | P2 | disk-io | **done** | Preallocation is not implemented |
| [T-013](disk-io.md) | P2 | disk-io | **done** | Selecting a subset of files still creates all of them |
| [T-014](disk-io.md) | P2 | disk-io | **done** | Adding a torrent can fail with "File exists (os error 17)" |
| [T-015](disk-io.md) | P1 | disk-io | **done** | Hash checking can hang at 0 percent |
| [T-016](disk-io.md) | P2 | disk-io | done | fastresume is not used when adding a torrent |
| [T-017](disk-io.md) | P1 | disk-io | **done** | Concurrent receive paths contend on the payload file |
| [T-018](disk-io.md) | P2 | disk-io | done | The write path issues one operation per 16 KiB block |
| [T-020](peers.md) | P0 | peers | **done** | Connections accumulate in CLOSE_WAIT until TCP is unusable |
| [T-021](peers.md) | P0 | peers | **done** | A temporary network drop stops the download permanently |
| [T-022](peers.md) | P1 | peers | done | Peer connections churn on IPv6-only swarms |
| [T-023](peers.md) | P1 | peers | **done** | The listen port is chosen without checking both address families |
| [T-024](peers.md) | P2 | peers | **done** | Per-peer choke and unchoke history is not reported |
| [T-025](peers.md) | P3 | peers | done | PeerStatsFilterState is not exported, so the filter is built by JSON |
| [T-030](performance.md) | P0 | performance | **done** | Throughput collapses with several torrents at once |
| [T-031](performance.md) | P1 | performance | **done** | The rate limit did not apply to the session |
| [T-032](performance.md) | P1 | performance | **done** | The piece selector strategy is not implemented |
| [T-033](performance.md) | P3 | performance | **done** | --split, -x, and -k do not reach the fetch path *(title disproved: they did not exist, and now do)* |
| [T-034](performance.md) | P3 | performance | open | Endgame mode is not observable |
| [T-035](performance.md) | P1 | performance | **done** | The web seed rate limit was never applied |
| [T-036](performance.md) | P0 | paths | **done** | A multi-file torrent with one file lands without its directory |
| [T-037](performance.md) | P1 | performance | **done** | A run stalls for minutes, roughly once in fifty |
| [T-040](memory.md) | P0 | memory | done | Memory and descriptors grow without bound over a long run |
| [T-041](memory.md) | P2 | memory | **done** | Per-source window cache is bounded but not measured |
| [T-042](memory.md) | P1 | memory | **done** | Peak RSS is not captured in any report |
| [T-050](dht.md) | P2 | dht | **done** | The DHT cache costs disk I/O even when nothing is running |
| [T-051](dht.md) | P2 | dht | **done** | A magnet with no DHT and no trackers fails without saying so |
| [T-052](dht.md) | P3 | dht | open | DHT is not reported |
| [T-060](trackers.md) | P1 | trackers | **done** | The announced port is wrong when no port is configured |
| [T-061](trackers.md) | P1 | trackers | **done** | bit-cli trackers announces a fixed port |
| [T-062](trackers.md) | P1 | trackers | **done** | Announce timing has no started, completed, or stopped events |
| [T-063](trackers.md) | P3 | trackers | **done** | Tracker tiers are announced in parallel rather than in order |
| [T-064](trackers.md) | P2 | trackers | **done** | UDP tracker retry does not follow the BEP 15 backoff |
| [T-065](trackers.md) | P3 | trackers | **done** | Scrape is only implemented for the BEP 48 URL convention |
| [T-070](windows.md) | P1 | windows | **done** | A downloaded executable cannot be run until the process exits |
| [T-071](windows.md) | P0 | windows | **done** | Reserved device names in torrent paths are not sanitised |
| [T-072](windows.md) | P0 | windows | **done** | Case-colliding paths silently overwrite |
| [T-073](windows.md) | P1 | windows | **done** | Long paths are not tested |
| [T-074](windows.md) | P1 | windows | **done** | A false hash-check pass on empty files |
| [T-075](windows.md) | P2 | windows | **done** | PowerShell redirection encoding is not documented |
| [T-076](windows.md) | P2 | windows | **done** | seed and verify do not report renamed paths |
| [T-080](create-seed.md) | P1 | create | **done** | librqbit's create_torrent writes an extra piece hash |
| [T-081](create-seed.md) | P1 | create | open | BEP 52 v2 and hybrid torrents are not implemented |
| [T-082](create-seed.md) | P2 | seeding | open | BEP 16 superseeding is not implemented |
| [T-083](create-seed.md) | P2 | seeding | open | Seeding does not report choke state or disconnect reasons |
| [T-084](create-seed.md) | P0 | create | **done** | The create round trip has not been proven against another client |
| [T-085](create-seed.md) | P1 | create | **done** | Creation determinism is not proven across platforms |
| [T-090](bench.md) | P0 | bench | done | bit-cli bench is not implemented |
| [T-091](bench.md) | P0 | bench | **done** | Bench reports do not capture their environment |
| [T-092](bench.md) | P1 | bench | done | bench swarm has no synthetic load generator |
| [T-093](bench.md) | P2 | bench | **done** | --baseline comparison is not implemented |
| [T-094](bench.md) | P2 | bench | **done** | Trace output has no measured cost |
| [T-100](bep-coverage.md) | P2 | bep | **done** | BEP 6 fast extension is not implemented |
| [T-101](bep-coverage.md) | P3 | bep | open | uTP is available but untested *(title disproved: it was not reachable; `--transport` reaches it now and the latency half is unmeasured)* |
| [T-102](bep-coverage.md) | P3 | bep | open | BEP 55 holepunch is not implemented |
| [T-103](bep-coverage.md) | P2 | bep | **done** | Filenames that are not valid UTF-8 are refused *(title disproved: they are decoded lossily)* |
| [T-110](cli-surface.md) | P1 | cli | **done** | The --jsonl event stream is incomplete |
| [T-111](cli-surface.md) | P2 | cli | open | piece_verified and file_completed are derived from polling |
| [T-112](cli-surface.md) | P1 | cli | **done** | --log-file does not write or rotate anything |
| [T-113](cli-surface.md) | P1 | cli | **done** | Metalink is not implemented |
| [T-114](cli-surface.md) | P2 | cli | open | -i/--input-file batch input is not implemented |
| [T-115](cli-surface.md) | P2 | cli | **done** | Hooks do not fire for every documented trigger |
| [T-116](cli-surface.md) | P3 | cli | **done** | -O/--index-out cannot rename a file |
| [T-117](cli-surface.md) | P1 | cli | **done** | --schema-version has no schema behind it |
| [T-118](cli-surface.md) | P3 | cli | **done** | The short-flag table is not checked in CI *(title disproved: it is, by four tests)* |
| [T-120](licensing.md) | P1 | licensing | **done** | THIRD_PARTY.md is not generated |
| [T-121](licensing.md) | P1 | licensing | **done** | No cargo-deny configuration |
| [T-122](reference-map.md) | P2 | licensing | **done** | The copyleft and unlicensed reference trees are deleted |
| [T-130](multi-source.md) | P1 | webseed | **done** | A source cannot be told which statuses are worth retrying |
| [T-131](multi-source.md) | P1 | bench | **done** | The loopback file server cannot simulate a signed URL |
| [T-132](multi-source.md) | P1 | performance | done | The swarm cannot be rate limited separately from HTTP sources |
| [T-133](multi-source.md) | P1 | webseed | **done** | Two torrents holding the same file cannot share its bytes |
| [T-134](multi-source.md) | P2 | bep | open | v1 and v2 info hashes are not reconciled |
| [T-135](multi-source.md) | P2 | performance | open | Source selection cannot be steered by method or by priority at run time |
| [T-136](multi-source.md) | P2 | cli | **done** | Nothing states the end-to-end integrity guarantee |
| [T-137](multi-source.md) | P2 | webseed | **done** | A cooled-down source never comes back |
| [T-138](peers.md) | P2 | peers | **done** | A peer that comes back waits out a backoff that grows by six |
| [T-139](multi-source.md) | P1 | cli | **done** | A resumed download charges its existing bytes to the swarm |
| [T-140](multi-source.md) | P2 | webseed | **done** | A proven shared file is not turned into a source on its own |
| [T-141](webseed.md) | P1 | webseed | **done** | --web-seed-connect-timeout does not bound a connect that never answers |
| [T-142](peers.md) | P1 | peers | **done** | bit-cli peers never joined the swarm it was sampling |
| [T-143](multi-source.md) | P2 | webseed | **done** | A source cannot be attached to a torrent that has already started |
| [T-144](cli-surface.md) | P1 | ci | **done** | The MSRV job fails: the tree needs a newer rustc than it claims |
| [T-145](cli-surface.md) | P2 | ci | **done** | The macOS test job fails to link |
| [T-146](cli-surface.md) | P1 | ci | **done** | CI built a Windows binary against the dynamic C runtime |
| [T-147](windows.md) | P1 | windows | **done** | The rename reason differed by host, so two tests only passed on Windows |
| [T-148](bench.md) | P2 | bench | **done** | The peer probe test asserted an exit code inside its own retry loop |
| [T-149](bench.md) | P1 | bench | **done** | The last window of a leech bench was never counted |
| [T-150](cli-surface.md) | P2 | ci | **done** | Clippy pins a floating toolchain, so a Rust release can turn the tree red |
| [T-151](cli-surface.md) | P1 | ci | **done** | Only one of the three release targets was checked for static linking |
| [T-152](bench.md) | P1 | bench | **done** | A disk bench shorter than one sample interval reported no series at all |
| [T-153](cli-surface.md) | P3 | ci | open | Link speeds are not read on macOS |
| [T-154](cli-surface.md) | P2 | cli | **done** | A Metalink named by URL is not recognised |
| [T-155](cli-surface.md) | P3 | cli | **done** | --hash-check-only drops the metalink report |
| [T-156](cli-surface.md) | P3 | cli | **done** | A dry run writes a different shape under the same document kind |
| [T-157](memory.md) | P2 | memory | **done** | A killed soak destroys the summary it was rewriting |
| [T-158](cli-surface.md) | P2 | cli | **done** | Regenerating the schema deletes fields the sample did not produce |
| [T-159](cli-surface.md) | P3 | cli | **done** | Subcommand flags are filed under "Report options" in the help |
| [T-160](cli-surface.md) | P1 | ci | **done** | A peers test raced its own seeder |
| [T-161](cli-surface.md) | P3 | ci | **done** | A CI action still targets Node.js 20, which is deprecated *(four call sites, not two)* |
| [T-162](webseed.md) | P1 | bench | **done** | Two bench webseed tests assumed a loaded runner cannot also fail |
| [T-163](peers.md) | P2 | peers | **done** | MSE/PE peer encryption is not implemented |
| [T-164](peers.md) | P2 | peers | partial | A peer that sends garbage keeps its connection slot |
| [T-165](peers.md) | P2 | peers | **done** | The peer's reqq is ignored, so the queue depth is a fixed 128 *(title disproved: it is read, and the depth follows it)* |
| [T-166](peers.md) | P1 | peers | **done** | BEP 10 extension ids are not proven to map in both directions |
| [T-167](bep-coverage.md) | P2 | bep | **done** | BEP 54 lt_donthave is not implemented |
| [T-168](bep-coverage.md) | P3 | bep | open | WebTorrent peers and WSS trackers are not supported |
| [T-169](dht.md) | P3 | dht | open | BEP 33 DHT scrape and BEP 51 infohash indexing are not implemented |
| [T-170](dht.md) | P3 | dht | open | BEP 44 mutable items are not implemented |
| [T-171](metainfo.md) | P2 | metainfo | **done** | httpseeds written as a bencoded string is silently dropped |
| [T-172](metainfo.md) | P2 | metainfo | **done** | Strictness on read is undecided, and the error does not say |
| [T-173](metainfo.md) | P3 | metainfo | **done** | A zero-length path component has no defined meaning *(title disproved: it is dropped, and the drop is reported now)* |
| [T-174](metainfo.md) | P2 | metainfo | **done** | A piece length that is not a multiple of 16 KiB has no fixture |
| [T-175](create-seed.md) | P2 | create | open | create does not normalise NFD filenames |
| [T-176](create-seed.md) | P2 | create | **done** | Three lints the corpus names are missing, and one message is wrong |
| [T-177](disk-io.md) | P2 | disk-io | **done** | A piece that spans a file boundary has no adversarial fixture |
| [T-178](windows.md) | P3 | windows | **done** | librqbit's Windows pwrite_all can spin forever on a zero-byte write |
| [T-179](webseed.md) | P2 | webseed | **done** | A bad piece cannot be attributed to the source that filled it |
| [T-180](trackers.md) | P2 | trackers | **done** | A negative left in a tracker exchange has no decided handling |
| [T-181](cli-surface.md) | P1 | cli | **done** | Four flags are accepted in silence and reach no code |
| [T-182](cli-surface.md) | P1 | ci | **done** | A macOS test asserted an invariant across two kernel subsystems |
| [T-183](cli-surface.md) | P1 | cli | **done** | --web-seed-list-url is read, only into a refusal |
| [T-184](disk-io.md) | P2 | disk-io | **done** | A boundary piece under --select-file has no decided behaviour |
| [T-185](cli-surface.md) | P1 | cli | **done** | --exclude-file on its own selects nothing and downloads everything |
| [T-186](cli-surface.md) | P3 | cli | **done** | seed --data and verify --data resolve the payload differently |
| [T-187](metainfo.md) | P3 | metainfo | **done** | Non-canonical integers are refused everywhere, with no instance behind the rule |
| [T-188](disk-io.md) | P3 | disk-io | **done** | A chunk starting on a file boundary creates the file before it |
| [T-189](bench.md) | P2 | bench | done | The bench reports are not in the schema contract |
| [T-190](disk-io.md) | P2 | disk-io | done | The rule for where a payload lands says one thing and the code does another |
| [T-191](bench.md) | P2 | bench | **done** | Two different documents answer to kind seed |
| [T-192](disk-io.md) | P2 | disk-io | open | What the write buffer is worth depends on what is above it |
| [T-193](cli-surface.md) | P2 | cli | done | A citation written short was never checked at all |
| [T-194](peers.md) | P0 | peers | **done** | A torrent past 131,960 pieces cannot be served or fetched at all |
| [T-195](peers.md) | P2 | peers | done | The read side caps the same message at 262,104 pieces |
| [T-196](cli-surface.md) | P2 | cli | done | A magnet that never resolves hangs download with no diagnostic |
| [T-197](cli-surface.md) | P1 | cli | **done** | Running upstream's tests filled the patch series with 14,964 patches |
| [T-198](cli-surface.md) | P1 | cli | **done** | An agent that wants a flag name greps for it |
| [T-199](cli-surface.md) | P2 | cli | **done** | The CI supply chain was unwatched and one action was abandoned |
| [T-200](phase-c.md) | n/a | phase-c | deferred | Session daemon |
| [T-201](phase-c.md) | n/a | phase-c | deferred | JSON-RPC and XML-RPC, with aria2 method parity |
| [T-202](phase-c.md) | n/a | phase-c | deferred | Queue management across invocations |
| [T-203](phase-c.md) | n/a | phase-c | deferred | Session save and restore |
| [T-204](phase-c.md) | n/a | phase-c | deferred | Persistent attached web seeds |
| [T-205](phase-c.md) | n/a | phase-c | deferred | Download result registry |
| [T-206](phase-c.md) | n/a | phase-c | deferred | GID assignment |
| [T-207](phase-c.md) | n/a | phase-c | deferred | Session-attached verbs from the old TUI |
| [T-208](phase-c.md) | n/a | phase-c | deferred | status --follow against a live session |
| [T-209](phase-c.md) | n/a | phase-c | deferred | Watch directories, RSS, cluster mode, and the control service |
| [T-210](peers.md) | P1 | peers | done | An incoming peer is recorded under this session's own peer id |
| [T-211](bench.md) | P1 | bench | **done** | Two bench tests fail on the CI runner and pass on every local run |
| [T-212](memory.md) | P2 | memory | open | Resolving a magnet can allocate 4 GiB across 128 peers |
| [T-213](cli-surface.md) | P3 | cli | **done** | seed cannot serve a payload renamed by --index-out |
| [T-214](cli-surface.md) | P3 | cli | **done** | seed runs no hooks |
| [T-215](webseed.md) | P1 | bench | **done** | A third bench webseed test asserted a loaded runner cannot fail |
| [T-216](windows.md) | P1 | ci | **done** | A seeder test waited longer for a listener than the run was allowed to live |
| [T-217](windows.md) | P2 | ci | **done** | The text gate caught one control byte and not the other twenty-eight |
| [T-218](cli-surface.md) | P2 | ci | **done** | The next stable release fails the build on a method the bridge calls |
| [T-219](cli-surface.md) | P1 | cli | **done** | Ten of the eleven trace subsystems raise a target nothing writes to |
| [T-220](cli-surface.md) | P2 | ci | **done** | The record gate reported on a tree the same run then rewrote |
| [T-221](windows.md) | P1 | ci | **done** | A seeder fixture treated a bound port as a session ready to answer |
| [T-222](cli-surface.md) | P1 | cli | **done** | A config file reaches `config show` and nothing else |
| [T-223](bench.md) | P1 | bench | **done** | The leech bench reads its transfer counters before deciding to stop |
| [T-224](memory.md) | P2 | memory | **done** | The six hour soak's RSS slope is one step and a sawtooth, not a leak *(the step does not reproduce, and the sawtooth is what a second rate shows)* |
| [T-225](create-seed.md) | P1 | ci | **done** | The interop script hashes files the client it just killed still holds |
| [T-226](cli-surface.md) | P1 | cli | **done** | `download --out` is parsed and never read |
| [T-227](memory.md) | P2 | memory | open | The window cache budget is per source, so the total is whatever the source count makes it |
| [T-228](cli-surface.md) | P3 | ci | **done** | Two gate runs at once fail on a locked file rather than on being two |
| [T-229](bench.md) | P1 | bench | **done** | A concurrency sweep charged its warmup to its own first steps |
| [T-230](cli-surface.md) | P1 | ci | **done** | A run's output reached the remote because nothing said what belongs here |
| [T-231](memory.md) | P1 | memory | **done** | A soak killed mid-write reads as a final sample of zeros |
| [T-232](memory.md) | P1 | memory | **done** | A six hour soak reported a pass on a workload that stopped after 78 minutes *(the stop never reproduced; what closed it is the listener figures reaching the report)* |
| [T-233](peers.md) | P1 | peers | open | MSE over uTP stalls after the handshake |
| [T-234](peers.md) | P2 | peers | open | bit-cli cannot present itself as a client a restrictive peer will talk to |
| [T-235](trackers.md) | P1 | trackers | **done** | Nothing compares the numbers a tracker sees against the run that made them |
| [T-236](peers.md) | P1 | peers | **done** | bit-cli announces under two peer ids and neither one is bit-cli *(six of them, and five said BitComet)* |
| [T-237](trackers.md) | P2 | trackers | **done** | Three announce paths have no fidelity case *(the third one was hiding [T-256](trackers.md))* |
| [T-238](peers.md) | P2 | peers | open | NAT traversal beyond the BEPs, and what a relay would actually buy |
| [T-239](peers.md) | P2 | peers | open | Nothing says what shape of network bit-cli is on, or whether a peer path is direct |
| [T-240](dht.md) | P3 | dht | open | A DHT node that answers slowly or emptily is queried again at the same rank |
| [T-241](metainfo.md) | P2 | metainfo | **done** | A resolved magnet keeps the payload and loses the metainfo *(nine commands take a magnet now, not one)* |
| [T-242](performance.md) | P2 | performance | open | The request depth is a constant, and the run sits at 40 percent of it |
| [T-243](phase-c.md) | n/a | phase-c | deferred | A user interface, and which of the two kinds decision 7.4 permits |
| [T-244](cli-surface.md) | P2 | cli | **done** | A web page is not a source, and nothing extracts a link from one |
| [T-245](cli-surface.md) | P1 | cli | **done** | Four commands refuse the URL download accepts *(nine of them, and a metalink too)* |
| [T-246](cli-surface.md) | P2 | cli | **done** | Three inputs report a file error and two of them name the wrong cause *(all three exit 2 now, where no source input could)* |
| [T-247](cli-surface.md) | P2 | cli | **done** | A dry run over a URL prints zero for a count it never took |
| [T-248](metainfo.md) | P2 | metainfo | open | There is no way to ask what two torrents disagree about |
| [T-249](metainfo.md) | P3 | metainfo | **done** | A torrent's shape is only ever printed as a flat list *(the span alone does not say a subtree stands alone, so `shared_pieces` sits beside it)* |
| [T-250](cli-surface.md) | P2 | cli | open | Nothing reports how an input was resolved |
| [T-251](trackers.md) | P2 | trackers | partial | A web seed has twelve knobs of its own and a tracker has none *(the source half [T-245](cli-surface.md) left here is done)* |
| [T-252](cli-surface.md) | P3 | cli | **done** | The run's numbers exist in JSON and cannot be asked for as text *(the disk half was plumbing, not a measurement)* |
| [T-253](cli-surface.md) | P2 | cli | partial | The schema sample takes one path, so thirteen real fields went undocumented |
| [T-254](webseed.md) | P2 | webseed | **done** | No report carries a response header, so a CDN cache hit is invisible |
| [T-255](cli-surface.md) | P2 | cli | **done** | Regenerating the schema deletes four hand-written sections and nothing fails |
| [T-256](trackers.md) | P1 | trackers | **done** | A UDP announce sends its event on every request, where an HTTP one sends it once |
| [T-257](cli-surface.md) | P2 | cli | **done** | Two documents answer to type progress, and the guard against that only covers documents *(session_start and session_end were being unioned too)* |
| [T-258](cli-surface.md) | P2 | cli | **done** | A seeder re-sends every peer it has ever seen, every report interval *(1.6 percent of the stdout after, on the same workload)* |
| [T-259](cli-surface.md) | P3 | cli | **done** | The schema's prose is generated and nothing compares it to what is committed |
| [T-260](cli-surface.md) | P2 | ci | open | A release publishes binaries and nothing a program can consume |
| [T-261](trackers.md) | P2 | trackers | open | There is no way to get a current tracker list, so every torrent carries whatever it was born with |
| [T-262](cli-surface.md) | P3 | cli | **done** | The HTTP/2 fingerprint matches a real Chrome in three fields of four |
| [T-263](cli-surface.md) | P3 | cli | **done** | The extension list is Chrome's set in a fixed order, and Chrome shuffles it |
| [T-264](cli-surface.md) | P2 | cli | partial | The browser profile can only be refreshed on a machine that runs that browser |

## Counts

213 items: 202 to work through, and 11 deferred to Phase C.
29 open, 4 partial, 0 blocked, 169 done.

Counted from the rows above by `scripts/check-todo.ps1`, which fails a gate
when a number here disagrees with them.

| Priority | Open | Partial | Blocked | Done | Total |
| --- | --- | --- | --- | --- | --- |
| P0 | 0 | 0 | 0 | 12 | 12 |
| P1 | 2 | 0 | 0 | 70 | 72 |
| P2 | 18 | 4 | 0 | 63 | 85 |
| P3 | 9 | 0 | 0 | 24 | 33 |
| Phase C | | | | 11 deferred | 11 |
| **All** | **29** | **4** | **0** | **169** | **213** |

`blocked` is zero and has been since 2026-08-22. Two entries were blocked on
`librqbit` and vendoring it removed the blocker, which is what vendoring was
for. One entry, [T-164](peers.md), carries a blocker and is `partial` rather
than `blocked`, because one of its three parts shipped.

## How the current ordering is derived

Four questions, asked in this order, because a later answer never outranks an
earlier one. This is the argument, not the list: [PROGRESS.md](PROGRESS.md)
carries the ordered work.

### 1. Is anything wrong that reports success?

A wrong answer that exits 0 outranks a visible failure, because nothing in the
output says the wrong answer happened. Two entries are in this shape now.

[T-232](memory.md) is the worked example and it is why the question is first: a
six hour soak reported "every ceiling held" over a workload that had stopped
after 78 minutes, and every number in the report was true. The instrument is
fixed and the cause is not.

[T-236](peers.md) is the other: `bit-cli` announces under two peer id prefixes
and neither is its own, one of them being BitComet's. Nothing fails, and every
tracker's client statistics are wrong.

### 2. Is a measurement blocked on something?

An entry that cannot be measured cannot be closed, so the thing that unblocks a
measurement outranks the measurement. [T-101](bep-coverage.md) is open on a
latency figure loopback cannot produce. [T-238](peers.md) is open on a NAT
shape loopback does not have, and [T-239](peers.md) is what would name it,
which is why T-239 outranks T-238 despite being the smaller idea.

### 3. What does a category pass close cheapest?

Entries cluster by file because they share a fixture. Taking a category at a
time means the fixture is built once. `bep-coverage.md` is first because
[T-101](bep-coverage.md), [T-102](bep-coverage.md) and
[T-168](bep-coverage.md) are the three oldest open entries in the list, then
`dht.md`.

### 4. What is waiting on a decision rather than on work?

These cost nothing to carry and nothing to close once ruled on, so they are
last in effort and first to raise. [T-033](performance.md) has its curve
measured and needs three flag names. [T-227](memory.md) needs one throughput
curve. [T-234](peers.md), [T-238](peers.md) and [T-243](phase-c.md) each carry
a recommendation and a question for the operator.

### What this ordering changed

The previous derivation, of 2026-08-21, put a silent wrong answer in the web
seed path above the visible P0 items. All four entries it named closed, and the
argument held for the first and did not survive the other three: two of them
recommended work the code had already made unnecessary.

So question 1 keeps the priority and question 2 is new. It was added on
2026-08-24 because three of this session's entries are open on a measurement
rather than on an implementation, and nothing in the previous derivation ranked
that shape at all.

The full 2026-08-21 derivation is in `reference/HISTORY/INDEX-history.md`.
