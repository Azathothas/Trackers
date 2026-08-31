# Adopt the template into an existing repository

⭐ **This is the one to paste into the agent of a project that already exists**,
however large and however messy. The agent fetches what it needs from the public
template over HTTPS. Nothing is cloned, nothing is installed, and nothing is
overwritten.

It differs from [`01-existing-project.md`](01-existing-project.md), which
assumes the template's files are already sitting beside the project. This one
assumes nothing but network access.

---

```text
Fetch and read this in full before doing anything else:

    https://raw.githubusercontent.com/Azathothas/TEMPLATE/main/ADOPT.md

It is self-contained. It carries the safety contract, the procedure, and a
manifest of what else to fetch and when. Fetch nothing beyond it until you have
read it and know which parts apply here.

YOUR TASK: bring this repository under that template, without making a mess of
it.

⛔ THE SAFETY CONTRACT IS NOT NEGOTIABLE, and it is in that file. The short
form, so you know before you fetch:

  1. Work on a new branch. Never the default branch.
  2. Never overwrite an existing file. Write the template version beside it
     with a .template-new suffix and show me the diff.
  3. Never delete anything.
  4. Never rewrite history. No rebase, no amend, no force push, no filter.
  5. Never commit until I have seen the diff.
  6. This project's existing conventions WIN. You are adding what is missing,
     not replacing what works.
  7. Nothing runs that writes outside this repository.
  8. A found secret is reported, never fixed silently. Rotation is mine.

WHAT I WANT BACK, IN THIS SESSION:

  Phase 0, measure and change nothing. Run the probe and the checks against
  this repository AS IT IS. Expect them to fail loudly. That is the output, not
  an error: a first run with forty findings has done its job.

  Phase 1, report and STOP. A findings list ranked by consequence, each with
  what it is, why it matters as a concrete consequence, its severity, the fix
  and what it costs, and the alternative including "leave it and write it down
  as accepted". ⭐ Anything the secret sweep found goes at the top on its own.

  Then propose an adoption set sized to what this project actually is, and WAIT
  for my explicit yes. "Leave it" is a complete answer for any item.

⚠ Be honest about what you cannot verify. If you cannot build or run this
project, say so rather than reading the source and calling it verified.

⚠ The single most likely way to make this worse is to be helpful. A messy
repository looks like it is asking to be tidied. It is not. It is asking to be
measured, so I can decide what to tidy.

CONTEXT, fill in what you know and leave the rest blank:

What this project is:
Its current state:      <complete | partial | abandoned | broken | unknown>
What I want from this:  <just the checks? the conventions? the whole method?>
What must NOT change:
Known problems:
Is it a monorepo:       <if yes, which package should we prove this on first>
Does it already have:   <CI? a linter? a commit convention? a work tracker?>
```

---

## What you get back

Not a merged change. Adoption ends with:

- a **reviewable branch** that has been committed to nothing and merged nowhere;
- the **before and after** numbers from the checks, so you can see what moved;
- every **`.template-new`** file still awaiting your decision;
- what the agent **did not** adopt, and why;
- ⭐ anything the secret sweep found, restated at the top.

## If it goes wrong

Nothing was deleted, nothing reached the default branch, and no history was
rewritten. Deleting the branch undoes the whole thing.

```bash
git switch -
```

```bash
git branch -D template-adoption
```
