# ANSWERS.md

**This file is for you, the operator, not for the agent to fill in.**

Fill in what you know, delete nothing, and paste the whole thing into a fresh
agent session together with the prompt from
[`prompts/00-new-project.md`](prompts/00-new-project.md).

You cannot fill it in wrong. Every field has a default, and a blank field means
"propose one and tell me what you picked". The agent asks you about a blank
field only when the two possible readings would produce genuinely different
projects. Everything else it defaults and reports.

The three fields worth a sentence each are **What it is**, **Audience and
scale**, and **Visibility**. Those three decide more of the shape than all the
rest combined. The rest can be blank.

⚠ Do not put a secret in here. Not a token, not a password, not a connection
string, not "the key is hunter2". This text goes into a session transcript.
Name the secret and say you hold it; the agent will tell you where to put it.

---

```text
=====================================================================
PROJECT

Name:
What it is:            <one line is enough>
Audience and scale:    <just me / me and a few friends / a team / public
                        service with N users. This decides the architecture
                        more than anything else here.>
Goals:                 <what success looks like. Blank = agent proposes.>
Non-goals:             <what this must not become. Blank = agent proposes.>

VISIBILITY AND REMOTE

Visibility:            public | private | local-only
                       <blank = private. A public repo changes what may go in
                        the tree, what the licence has to say, and whether CI
                        is free.>
Remote:                <the URL, or "none yet", or "I will create it and tell
                        you later". Blank = local-only, no remote configured.>
Who creates the repo:  me | you
Push policy:           commit-only | commit-and-push | ask-each-time
                       <blank = commit-only. The agent commits freely and
                        locally and never publishes. Raise it deliberately.>

WORK MODEL

Model:                 stage | todo | you-choose
                       <blank = you-choose. The agent picks from the shape of
                        the work and says why. Roughly: nothing exists yet and
                        the work is a dependency-ordered build, use stage. A
                        tree that already exists and a backlog to prioritise,
                        use todo. docs/methodology/choosing-a-work-model.md is
                        the full rule.>

Agent-facing files:    keep | none
                       <blank = keep. "none" takes the checks, the conventions,
                        the probe and the dotfiles and ships NO file addressed
                        to a machine: no router, no methodology directory, no
                        skeletons. Pick it for a compliance rule, or because
                        you do not want them, which is a complete reason.
                        docs/methodology/lean-adoption.md is what happens then.
                        Decide now: it is a selection, and stripping it later
                        leaves a history full of what you did not want.>

ENVIRONMENT

Host:                  <blank = this machine, and the agent measures it. Fill
                        this only if the project runs somewhere else: a server,
                        a container, WSL when you are on Windows, a device.>
Language or stack:     <what you require. Blank = agent recommends and asks
                        before locking it.>
Forbidden:             <anything you do not want used, and why if it matters>
CI:                    yes | no | later
                       <blank = yes for public, no for private. Actions minutes
                        are free on public repos and billed on private ones.>

LEGAL

Licence:               <MIT | Apache-2.0 | AGPL-3.0 | MPL-2.0 | Unlicense |
                        proprietary | none. Blank = MIT for public, none for
                        private.>
Copyright holder:      <blank = taken from your git config. Nothing is invented
                        and nothing is hardcoded in the template.>

CONSTRAINTS AND CONTEXT

Hard constraints:      <budget, deadline, a platform it must fit, compliance,
                        an existing system it must not break>
Already decided:       <so the agent does not re-litigate it. It will still
                        tell you if one of these is a mistake.>
References:            <links, repos, local directories, prior art. Say what to
                        learn from each and what to ignore. "Build it like X
                        but without Y" is the most useful form.>
Unknowns:              <what you want help figuring out>
Questions for me:      <anything you want answered before planning>

SECRETS AND ACCOUNTS

I hold:                <name them, never their values. "A Cloudflare API token",
                        "the deploy SSH key". The agent tells you where each
                        goes and never asks for the value.>
Not available:         <credentials, environments or services the agent cannot
                        reach, so it marks what it cannot verify instead of
                        assuming it>
=====================================================================
```

---

## After you paste this

The agent runs the probe, reads this, and comes back with:

1. What it measured about your machine, and anything here that the measurement
   contradicts.
2. The decisions it defaulted, listed, so you can object to any of them.
3. The genuine forks it cannot default, asked as one grouped question with a
   recommendation attached to each, so agreeing to all of it costs you nothing.
4. What it will delete from the template and what it will keep.

It does not write project code in that session. The bootstrap produces a
configured repository, a plan, and the first unit of work, and it stops for
your approval before implementing anything.

⛔ **`bootstrap/` is deleted at the end of the bootstrap**, this file with it.
[`README.md`](README.md) says why this directory goes: a started project does
not need the instructions for starting. If you want to keep your filled-in
answers, the agent copies them into the project's own record first, with the
secrets section stripped.
