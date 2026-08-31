# LICENSES

Canonical licence texts, and a script that fills one in without breaking it.

⭐ **Every text here was fetched from the SPDX license-list-data repository, not
written from memory.** A licence reproduced approximately is a legal defect that
nobody notices until it matters. If an id you need is missing, fetch it the same
way rather than typing it:

```bash
curl -sSL -o LICENSES/MPL-2.0.txt https://raw.githubusercontent.com/spdx/license-list-data/main/text/MPL-2.0.txt
```

---

## ⛔ Use a script, not a text editor

⭐ **The filler lives upstream now**, and
[`../docs/agent-tooling.md`](../docs/agent-tooling.md) says where. It takes an
identifier and a holder, reads `git config user.name` when given no holder, and
⛔ **invents nothing**: with no name configured it refuses rather than guessing,
and no name is ever baked into this template.

⚠ **The reason the texts stayed here and the filler did not** is that a
project must be able to write its `LICENSE` from a canonical text it already
has. The texts are canonical SPDX and do not change; a script does.

---

## ⛔ Why a naive fill script corrupts five of these twelve

This is the reason the script carries a table instead of a regex. Each of these
was found by reading the actual texts.

| licence | what goes wrong |
| --- | --- |
| **GPL-3.0, AGPL-3.0, LGPL-3.0** | The copyright at the top belongs to the **Free Software Foundation**, on the licence document itself. It is not yours. Rewriting it is wrong and is a licence violation. Your notice goes in each source file's header. ⛔ The script refuses these. |
| **ISC** | SPDX ships a licence **instance**, carrying Internet Systems Consortium's own copyright. Shipping it unedited attributes your software to them. ⛔ The script refuses without an explicit override. |
| **0BSD** | Its placeholder is the bare word `AUTHOR`, and the same word appears twice more in its warranty clause. A global replace produced *"THE Test Holder DISCLAIMS ALL WARRANTIES"*. ⚠ **This actually happened while building this directory**, and the placeholder check passed over it, because it only ever asked whether a placeholder survived. The substitution is now anchored to the first line, and a second guard asserts that no line other than the notice changed. |
| **MPL-2.0, CC0-1.0, Unlicense** | No copyright line to fill at all. Copied verbatim. |

⭐ Four different placeholder styles appear across twelve files: `<year>`,
`[yyyy]`, the bare word `YEAR`, and none. Any script that assumes one style is
wrong about most of them.

---

## Choosing one

Not legal advice. This is the shape of the decision, so the question can be
asked properly.

| you want | look at |
| --- | --- |
| the simplest permissive licence, understood everywhere | **MIT** |
| permissive, plus an explicit patent grant and a trademark clause | **Apache-2.0** |
| permissive, and as short as legally possible | **0BSD**, **ISC** |
| changes to **these files** come back, but linking is free | **MPL-2.0** |
| changes to anything built on it come back | **GPL-3.0** |
| the same, and running it as a network service counts as distributing | **AGPL-3.0** |
| to give up copyright entirely | **Unlicense**, or **0BSD** |

⚠ **CC0-1.0 is not recommended for software.** It explicitly does not grant
patent rights, and several large organisations refuse it for that reason.

⚠ **Apache-2.0 and GPL-2.0 are incompatible.** If a dependency is GPL-2.0-only,
Apache-2.0 is not available to you. ⭐ Check what you depend on **before**
choosing, not after.

---

## What is here

| id | short | notes |
| --- | --- | --- |
| `MIT` | permissive | the default for most projects |
| `Apache-2.0` | permissive | explicit patent grant, trademark clause |
| `BSD-2-Clause` | permissive | |
| `BSD-3-Clause` | permissive | adds a no-endorsement clause |
| `0BSD` | permissive | no attribution required at all |
| `ISC` | permissive | ⛔ SPDX text is an instance, see above |
| `Unlicense` | public domain | |
| `CC0-1.0` | public domain | ⚠ not for software |
| `MPL-2.0` | weak copyleft | per file |
| `GPL-3.0-only` | copyleft | ⛔ do not edit the notice |
| `AGPL-3.0-only` | copyleft | ⛔ do not edit the notice. Network use counts. |
| `LGPL-3.0-only` | weak copyleft | ⛔ do not edit the notice. Ships with GPL-3.0. |
