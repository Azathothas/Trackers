# HUMAN.md

<!-- TEMPLATE. Fill every {{PLACEHOLDER}} and delete this comment.
     This file is for the OPERATOR, not the agent. The agent reads it only to
     keep it current. -->

Your side of {{PROJECT}}.

⭐ **One line: if it needs a login, a token, a payment, a domain or a judgement
call, it is yours. If it is code, tests or local verification, it is the
agent's.**

You own three things the agent cannot: **credentials and remote
infrastructure**, **validation** (you are the acceptance gate), and **session
management** (starting sessions with the right context, receiving handoffs).

---

## 1. Machine check

⭐ **This is your first task, and it exists so work does not stall three tasks
in on a missing tool.**

```bash
sh scripts/doctor/doctor.sh
```

That reports what is installed and at what version. What this project needs:

| tool | minimum | needed from | check |
| --- | --- | --- | --- |
| {{tool}} | {{version}} | {{when it becomes needed}} | {{the one-line command}} |

{{Anything not needed until later, say when, so you do not install it now.}}

---

## 2. Secrets you hold

⛔ **Never paste a value into an agent session.** Name what you hold; the agent
tells you where it goes.

| what | where it goes | who sets it |
| --- | --- | --- |
| {{the credential}} | {{the ignored file, or the platform's secret store}} | you |

⚠ The agent will not ask for a value, and will not set a platform secret it
cannot read back. If one of those needs doing, it appears in a handoff as an
item for you.

---

## 3. Prompts to paste

### Start the next unit of work

The agent prints this at the end of every session. ⭐ **Paste it into a fresh
session.** It carries the reading order and a pointer to the record; it does
not carry the work order, because that lives in the record and would go stale
here.

### Resume an interrupted session

The agent prints this instead whenever anything was left unfinished. Paste it
the same way. ⚠ The agent will reconstruct the state from the tree and the
record rather than trusting anything it is told, including by you.

### Add new work

```text
Read {{the router}} and {{docs/methodology/authoring.md}}. Author a plan from
this intake. ⛔ Do not implement.

Title:
Type:            bug | feature | refactor | hardening | polish | chore
What and why:
Evidence:        <file and line, an error, a report, a URL>
In scope:
Out of scope:
Constraints:
Already decided:
```

### Reject a unit of work

```text
{{The unit}} is NOT accepted. What I found during validation:

1. What I did: ...
   What happened: ...
   What I expected: ...
2. ...

Reproduce each one first. Fix the root cause, not the symptom, and no
workarounds. Add a regression test per issue. Re-run the full gate. Update the
handoff. Do not proceed to anything else.
```

### Answer a design question

```text
Decision: <your decision in one sentence>.
Proceed with that. Record the question, my decision and the implications in the
handoff's deviations section. Do not revisit unless I raise it.
```

---

## 4. Receiving a handoff

Fifteen minutes, and it is worth all fifteen:

1. Read the items at the top. Do them, or schedule them.
2. ⭐ **Run its "verify the current state from scratch" commands yourself.** All
   of them. That section exists for exactly this.
3. Spot-check two or three acceptance items by hand.
4. Read the self-review answers. ⚠ **Red flags: "mostly", "should", an
   unanswered question, and a checklist item with no pasted output.**
5. Read the driven-pass log. ⚠ If it describes intent rather than what actually
   happened, that is a failed gate.
6. Accept, or reject with the prompt above.

---

## 5. Validating each unit of work

{{Per unit: what to click, what to check, and what a correct result looks like.
Fill this in as the plan is written, not afterwards.}}

| unit | what to validate | what correct looks like |
| --- | --- | --- |
| {{id}} | {{what you do}} | {{what you should see}} |

---

## 6. Remote runbooks

{{The things only you can do, with the exact steps. Console clicks, account
setup, DNS, billing, key rotation. Assembled from every unit's operator items,
so this becomes the deployment runbook over time.}}

---

## 7. Ground rules to hold

- **One unit of work per session chain.** The agent hands you a prompt at each
  boundary; paste it into a fresh session.
- ⭐ **A deviation from a locked decision is your call, never the agent's.** It
  will ask. Answer in one sentence and it records the ruling.
- ⛔ **"Should work" without pasted output is not done.**
- **If it says it is blocked on you, check its handoff items.** Either do the
  item, or tell it to mock and continue.
- ⚠ **If the agent accepts everything you say without a single question or
  counter-proposal, it is not doing the job.** The method is explicitly not a
  yes-machine.

---

## 8. Troubleshooting

| symptom | first thing to check |
| --- | --- |
| {{symptom}} | {{what to check}} |
