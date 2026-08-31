# The manuals, and why there are three of them

`man/` holds the command surface in three shapes. All three are **generated
from the clap definition** and all three are **committed**, so a reader can
open them without building anything.

| file | for | what it is |
| --- | --- | --- |
| [`man/bit-cli.1`](../man/bit-cli.1) | a person at a terminal | troff, the top level then one section per subcommand |
| [`man/bit-cli.md`](../man/bit-cli.md) | a person reading on the web, and an agent that wants prose | the same manual as Markdown, with a table per command |
| [`man/bit-cli.json`](../man/bit-cli.json) | a program | a [CLIspec](https://github.com/rvben/clispec) 0.3 document |

## Read these before guessing a flag

**This is the point of the exercise.** An agent that needs a flag name has two
options: read the surface, or guess. Guessing costs a run that exits 2, or
worse, one that succeeds having done something else. `--help` output is written
for a human and is expensive to page through one subcommand at a time;
`man/bit-cli.json` is one file, indexed by command, and it answers in one read.

For any question of the form "what is the flag for X", "what values does Y
take", "what does exit code N mean", or "is it safe to run this twice":

```bash
python -c "import json;d=json.load(open('man/bit-cli.json'));print([c['name'] for c in d['commands']])"
```

The document carries, for every command and every flag:

- `name`, `short`, and `value_name`, so the spelling is never guessed.
- `type`: `string`, `boolean`, `integer` or `array`. An `array` is repeatable.
- `enum`, where clap knows the accepted values. **This is the field that stops
  a caller inventing a value for an enum flag.**
- `default`, where there is one.
- `description`, the same help text the man page carries, on one line.
- `effects` per command: `read_only`, `idempotent` or `non_idempotent`. An
  agent deciding whether it may retry after a timeout reads this.
- `errors`: every exit code, its `kind`, what it means, and whether a second
  attempt could succeed.

## They cannot go stale

`cargo test -p bit-cli --test man_is_current` renders all three from the crate
being compiled and compares them to the committed files. A flag that is added,
renamed or removed fails the build until they are regenerated. That test is in
`cargo test --workspace`, so it is in the gates and in CI, on every platform CI
builds.

```bash
pwsh -NoProfile -File scripts/check-man.ps1 -Fix
```

That regenerates all three. `scripts/gates.ps1` also runs
`scripts/check-man.ps1` as a named `man` gate, which is there so a session gets
told what to run rather than reading a test name out of a failure. The script
compares against `target/release/bit-cli`, which can be older than the source,
so **the test is the check that binds** and the script is the tool.

## What is generated and what is not

Everything is walked out of `Cli::command()` and out of `ExitCode::ALL`, with
one exception: `effects` cannot be derived, because nothing in a clap
definition says whether a command writes. It is a table in
`crates/bit-cli/src/cmd/spec.rs`, and a subcommand with no entry in it fails
`every_subcommand_is_classified` rather than shipping with an empty `effects`,
which a reader would take to mean "no side effects".

The Markdown is rendered **from the CLIspec document** rather than from clap a
second time, so the two cannot disagree about what a flag is called or what it
accepts.

## Two bugs this shape caught immediately

Both were in the first generated document, and both are the kind a reader would
have believed:

- **`--web-seed` was typed `boolean`** while carrying `value_name: URL`.
  `clap::Arg::get_num_args` is empty until the command is built, so the whole
  surface reported flags that take values as flags that do not. It is read from
  the action now, and `render` builds the command first.
- **`create --version` disappeared.** Filtering clap's generated `--version` by
  argument id also deleted the metainfo version flag, which takes `v1`, `v2` or
  `hybrid`. Filtered by action now.

## Generating one by hand

```bash
bit-cli man                      # troff, to stdout
bit-cli man --format markdown    # Markdown
bit-cli man --format json        # CLIspec
bit-cli man --format json -o man/bit-cli.json
```
