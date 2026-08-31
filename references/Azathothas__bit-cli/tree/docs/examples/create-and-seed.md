# Making a torrent and seeding it

End to end, with the output every step actually produced. The payload here is
three files, 1.47 MiB in total, in a directory called `payload`.

## Create

```bash
bit-cli create payload --name album --piece-length 256KiB \
  --comment "example" --no-creation-date --output album.torrent --show
```

```text
name                 album
info hash            ae3ee993bf4fd886e98f15b899664c4d212085b2
output               album.torrent
size                 1.47 MiB
files                3
piece length         256.00 KiB
pieces               6
private              false
piece choice         256.00 KiB for 1.47 MiB of payload gives 6 pieces and 120 B of piece hashes
```

`--no-creation-date` is what makes the result byte reproducible: without it two
runs over the same bytes produce two different info hashes. `--no-created-by`
does the same for the `created by` field.

The `piece choice` line is the reasoning, not a summary. It is printed so a
choice that produces a 400 KiB `.torrent` is visible before anybody uploads it.

## Look at what was made

```bash
bit-cli info album.torrent
```

```text
name                 album
info hash            ae3ee993bf4fd886e98f15b899664c4d212085b2
size                 1.47 MiB
files                3
pieces               6 x 256.00 KiB
private              false
comment              example
created by           bit-cli/0.2.0
magnet               magnet:?xt=urn:btih:ae3ee993bf4fd886e98f15b899664c4d212085b2&dn=album&xl=1543000
```

```bash
bit-cli files album.torrent
```

```text
INDEX  SIZE       SHARE   PIECES  PATH
0      1.43 MiB   97.21%  0-5     album.flac
1      39.06 KiB  2.59%   5-5     cover.jpg
2      2.93 KiB   0.19%   5-5     extras/notes.txt
```

The `PIECES` column is the part worth reading before scoping a web seed: files
1 and 2 share piece 5 with file 0, so a mirror that holds only `cover.jpg`
cannot serve a whole piece on its own. `bit-cli webseed list` says the same
thing as `0 whole pieces, 1 partial`.

## Verify before seeding

```bash
bit-cli verify album.torrent --data payload
```

```text
torrent              album
info hash            ae3ee993bf4fd886e98f15b899664c4d212085b2
data                 payload
pieces ok            6 of 6
have                 1.47 MiB (100.00%)
complete             true
```

`--data` is where the payload already lives. Point it at the wrong directory
and the run exits **7** with `6 of 6 pieces failed` rather than reporting a
partial: a torrent whose every piece fails is a path problem, not a corruption
problem, and the exit code says which.

## Seed

```bash
bit-cli seed album.torrent --data payload --port 0 --seed-time 1h --jsonl
```

`--port 0` asks the operating system for a free port and the chosen one is
reported in the first `session_start` event, so a script can read it rather
than guessing. `--seed-time` bounds the run; `--seed-ratio` bounds it by ratio
instead, and whichever is reached first ends it.

A seeder holds no handle that stops another process running the file it is
serving, so a downloaded executable can be run while it is still being seeded.
That is [`../windows.md`](../windows.md).

## Prove another client can open it

A torrent this tree wrote that another client will not open is the failure
worth catching:

```bash
pwsh -NoProfile -File scripts/interop-roundtrip.ps1
```

That drives `aria2c` 1.37.0 and `rqbit` 9.0.1 against a torrent `bit-cli`
created, and a payload `bit-cli` seeded, in both directions. `-Client` selects
one.

## The lints, and why they refuse

`create` refuses two things another client will reject, and both are
overridable with `--allow <LINT>` when you know what you are doing:

- **`windows-path`**, a name Windows will not accept.
- **`case-collision`**, two names that differ only in case.

They are refusals rather than warnings because the failure they prevent happens
on somebody else's machine, after the torrent is public.
[`../create-seed.md`](../create-seed.md) has the argument.
