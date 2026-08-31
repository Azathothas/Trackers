# Security

<!-- TEMPLATE. Fill every {{PLACEHOLDER}} and delete this comment.

     ⭐ Writing this file IS a security audit. Being forced to state precisely
     who holds what, and what each leak reaches, is where the holes get found.
     Expect this pass to generate findings. That is the feature. -->

## The threat model

⛔ **State the audience, because it decides everything below.**

{{Who this serves, and at what scale. "One operator" and "a public multi-tenant
service" are different threat models from the same code, and a file that does
not say which is a file nobody can reason from.}}

**In scope:** {{the attacks this design defends against}}

**Explicitly out of scope:** {{the ones it does not, and why that is an
acceptable trade here. ⛔ Be honest. A limit hidden is a defect filed against a
user later.}}

⚠ {{If the audience assumption ever changes, this file is the first thing to
re-derive. Say so here, so a future session knows it is a live assumption and
not a settled fact.}}

---

## Who holds what

| secret | held by | where it lives | what it reaches if leaked |
| --- | --- | --- | --- |
| {{name}} | {{the operator / the platform}} | {{the store, never the value}} | ⭐ {{the blast radius, concretely}} |

⭐ **The blast-radius column is the one that matters.** "An API token" is not an
answer; "read and write on every object in the production bucket" is.

---

## The key hierarchy

{{What derives from what, and what a compromise at each level costs. If one
secret can mint another, say so: that is the difference between one incident
and all of them.}}

---

## Trust boundaries

{{Where untrusted input enters, and what validates it at each point. One entry
per boundary.}}

| boundary | what enters | what validates it |
| --- | --- | --- |
| {{e.g. the public API surface}} | {{what a caller controls}} | {{where, at file and line}} |

---

## The invariants

⛔ Non-negotiable. Each is a rule the code must hold, not an aspiration:

- {{e.g. every secret comparison is timing-safe}}
- {{e.g. passwords use a memory-hard function at the stated parameters}}
- {{e.g. every path into an operation passes the same gate}}
- {{e.g. nothing is ever logged that could reconstruct a credential}}

⚠ **When one of these turns out to be unbuildable on the platform, that is a
finding to raise, not a rule to quietly weaken.** The worked example: a project
specified an iteration count its deployment platform silently capped far below,
and shipped a broken authentication path for six units of work because nobody
surfaced it. Raising it and adopting a *stronger* alternative is what the rule
should have produced on day one.

---

## Reporting a vulnerability

<!-- Public repositories only. Delete this section for a private project. -->

{{Where to send it. What to include. What happens next, and roughly how long a
reporter should expect to wait.}}

⚠ **Do not promise a timeline you will not meet.** Without any answer here, a
finder's only options are a public issue or silence, and both are worse.

---

## Known limits

⭐ **The truths that are tempting to leave out.** Each one is here so a user
learns it from this file rather than from an incident.

- {{the limit, and why it is accepted}}
