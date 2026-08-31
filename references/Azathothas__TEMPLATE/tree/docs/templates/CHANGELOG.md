# Changelog

<!-- TEMPLATE. Delete this comment and the example section below.

     What shipped, when, and where the evidence is. The full rules are in
     docs/conventions/docs.md. The four that a check can hold:

       1. ⛔ NEWEST FIRST. A new entry goes at the TOP of its section, never
          appended to the bottom. This is the rule that breaks most often.
       2. ⛔ Every heading carries a date. Consider a full ISO 8601 UTC stamp:
          several entries sharing one date cannot be ordered from what was
          written down, and a single day spans several sessions.
       3. ⛔ Every entry names its record. An entry with no record is a claim.
       4. ⛔ Every entry says whether it deployed. "No version bump and no
          deploy" is a complete and common answer. Silence is not.

     And two things an entry must not do:
       ⛔ Do not tidy the file in the commit that adds to it. Tidying is its
          own commit, or both become unreviewable.
       ⛔ Do not delete an entry. A superseded one is amended in place with a
          dated note. Amend, never silently delete.

     ⚠ This file is EXPECTED TO GROW. It is the destination for what a
     documentation pass removes: when a document loses the story of a fix, the
     story comes here. Its length is not a defect. -->

{{One line on what this project versions, and what a version number is a
statement about. A semantic version is a claim about a specific surface; say
which surface.}}

---

## Unreleased

<!-- Work that has shipped code but has not been versioned. Same descending
     order inside. -->

### {{ISO 8601 UTC}}: {{one-line headline}}

**Record:** {{the handoff, plan or commit carrying the evidence}}.
⚠ {{Deployed, or explicitly not. Version moved, or explicitly not.}}

{{Two to six sentences, or a table. What changed and what it buys, with
numbers. ⛔ Do not restate a measurement's derivation: give the number and
point at the document that derives it.}}

---

## {{0.1.0}}: {{the first unit of work}}, {{one-line headline}} ({{YYYY-MM-DD}})

**Record:** {{path}}.
⚠ {{Deployed, or not.}}

{{What it did.}}
