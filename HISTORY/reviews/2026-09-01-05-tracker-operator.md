# Review 5, the tracker operator

**Date:** 2026-09-01. **Standpoint:** somebody who runs one of the trackers in
this corpus, has noticed traffic from this project, and is deciding whether to
be annoyed. They will read the README, not the rules, and they will check
whether what it says is true.

**What I looked for:** something this session changed about how the project
treats other people's servers, or about what it says it does. The previous
session ran this lens
([`2026-08-31-05-tracker-operator.md`](2026-08-31-05-tracker-operator.md)) and
found a README that promised a policy RULES 4.1 had withdrawn. This session
re-pointed the project's identity, re-ran live probes twice, and found that the
published dataset carries other people's credentials.

**What I did not look at:** the probe's ladder and its refusal vocabulary. That
was checked when it was written and nothing this session touched it.

---

## Findings

### 1. ⛔ The project publishes six people's private-tracker passkeys

Not an operator-conduct question in the usual direction. **This is the project
handing a stranger's credential to every consumer of its dataset**, and the
tracker that issued it sees every use.

Six distinct credentials, on seven URLs, reaching `trackers_all.txt` from two
upstreams that publish them. `C-70` records it, `T-107` fixes it, and
`check-no-secrets.py` holds the count so a seventh fails the gate.

⭐ **From an operator's side this is the most serious thing in the tree**, and
it is worth being precise about why: a passkey in an aggregated list is more
than a leak. It invites strangers to announce against a private tracker under
somebody else's identity, and the account that gets banned is the person whose
URL was pasted, not this project.

⚠ **The fix is not to redact.** A URL with the token stripped is an endpoint
that answers differently, and publishing it as though it were the tracker is
the invented-endpoint mistake `C-66` already cost this project once. The entry
says refuse, at the exclusion stage, with the reason recorded.

### 2. The probe still cannot announce, and that is still structural

`src/trackers/probe.py` has no announce path and `bep15.py` has no function
that builds one. Verified by reading rather than by trusting the comment.
Every committed result from this session's two live runs records
`announce_sent: false`.

⭐ **That is the difference an operator would actually notice**, and the
comparison is in the tree: `C-69` records that the closest production analogue
announces with `left=0`, which tells every tracker it probes that it is a seed
for a random infohash. This project never sends `left` at all.

### 3. The contact route in our User-Agent worked, and it looked like it did not

Every request to a tracker carried a User-Agent naming the project at
`github.com/Azathothas/trackers`, all lower case. The repository is
`Azathothas/Trackers`, with a capital.

⚠ **This is the shape of round 2's correction 7**, where the User-Agent named
an owner that did not exist, so it was checked rather than assumed: **both
spellings return 200**, because GitHub resolves repository names
case-insensitively. The contact route was never broken.

**Normalised anyway**, in all seven files that carry it, because an operator
copying the string into a browser should land somewhere that looks canonical,
and because the next reader should not have to re-run the check above.

### 4. Two live runs in forty minutes, and the load is worth stating

This session ran `p0-ground-truth.yml` twice: once to re-take a baseline whose
evidence was lost, and once more after finding that the instrument's verdict
was wrong.

Per run, per image: 11 UDP connects x 2 rounds, 6 TCP connects, 6 HTTP scrapes,
17 DNS lookups. Two images, two runs. ⚠ **That is roughly 180 requests spread
over four job executions to about 20 distinct hosts**, none of them repeated
back to back.

⭐ **The second run was not optional.** Publishing a baseline whose headline
verdict was wrong would have been worse than the requests, and RULES 2's
"run the control twice" is the rule that turned out to matter: the two runs
disagree about the DNS finding, and only having both makes that visible.

### 5. What an operator would still be entitled to ask, and cannot be answered

⚠ **Whether this project's identity gets it treated differently.** RULES 4.1
holds the question open, `C-64` measured a non-tracker intermediary refusing
this exact User-Agent while accepting `curl/8.5.0`, and `C-68` records that the
closest production analogue impersonates a real client on both identity axes.
`T-012` is the entry and it is the first item in the work order.

⛔ **Nobody has asked a tracker operator anything.** That was true at the last
review and it is still true. Two indirect signals exist and indirect is what
they are.

---

## What would have made this pass fire harder

The README was rewritten this session and now states the vantage limitation and
the open identity question before it states anything the project can do. ⚠ **An
operator reading it would find the credential finding under "known weaknesses"
rather than at the top**, which is a judgement I made and which somebody could
reasonably call wrong: it is the most serious thing here and it is fourth in a
list.
