# 2026-09-08-10 adversarial sweep

*Attack the code this session added, by running it rather than re-reading it.*
The lens `2026-09-05-01` used, aimed at the thing that changed most: the sweep
is **scheduled** now, so it runs unattended every three hours and nobody reads
its output between runs.

Four attacks. Three landed, and one of the three would have made the schedule
worthless within a day.

---

## Attack 1: give the rotation a corpus that changes size ⛔ landed, fixed

`ngosang` and `XIU2` regenerate daily, so the corpus one scheduled run sees is
not the corpus the last one saw. The rotation sliced by **position**:

```python
[t for i, t in enumerate(ordered) if i % slices == keep]
```

At 1327 trackers over 7 slices, adding **one** tracker shifts every index by
one, so slice 1 of the new corpus is *exactly* slice 0 of the old.

| trackers added | overlap between consecutive runs |
| --- | --- |
| +1 | **190 of 190 -- the identical set** |
| +5 | 0 |
| +20 | 0 |
| +100 | 24 |

⛔ **A daily +1 makes the schedule re-probe one slice forever**, which is the
defect the rotation was built to prevent, back again with a rotation bolted on
top. Fixed: `slice_of` decides membership from the tracker's own hash, so a
corpus that gains or loses entries moves nobody else between slices. Full
coverage over a pass is unchanged and asserted.

⚠ **My own first attempt at this attack returned a false negative.** I appended
`udp://new.example/...`, which sorts *after* every `h000nn` in the fixture and
therefore shifted nobody. The test that found it appended a name sorting
*before* them. A single hand-picked input is not an attack.

## Attack 2: two slices at one instant ⛔ landed, fixed

`sweep_identity` was `generated_at|mode|source|count`. Two rotations of the
same corpus at the same injected instant carry the same clock, the same mode
and the same record count, so the second is refused as **already folded** and
190 real observations are dropped.

A guard against double-counting that discards distinct data is RULES 3.9's
failure wearing the costume of a fix. The slice is part of the identity now.

## Attack 3: give the workflow a clock it cannot read ⛔ landed, fixed

`rotation_for` returned **0** for anything it could not parse. So a typo in the
workflow's date expression pins every scheduled run to slice 0 -- the same 190
trackers eight times a day, the other 1137 never, silently, and
indistinguishable from working. It raises now, and `probe-corpus.py` exits 2:
a run that cannot tell when it is has not been told.

## Attack 4: run the workflow and read what it did ⛔ landed, fixed

Dispatched run `34252497106`. It succeeded, and its own log carried the defect
four lines apart:

```
What this run would probe   rotation: slice 0 of 7 (rotation 0)
Sweep                       rotation: slice 3 of 7 (rotation 165637)
```

The dry-run step exists so that *"a sweep whose scale is only visible
afterwards is one nobody can decline"*. It passed no clock, defaulted to the
epoch, and previewed a **different 190 trackers** than the sweep contacted. One
instant is fixed once for the job now and both steps read it; a test refuses a
per-step `date` call as well as a differing one, because two calls agree almost
always and disagree across a three-hour boundary.

## What held

* `--only-host` and `--only-source` narrowing with a rotation: a corpus below
  the sample size is one slice and the rotation is a no-op.
* The `local` profile ignores the rotation entirely, at every rotation value.
* `slices_for` with a corpus smaller than the sample, and with a rotation far
  larger than the slice count.

For these to have fired, `select` would have had to apply the rotation before
the narrowing, or `slices_for` return zero and raise on the modulo.

## What this pass did not attack

* The probe itself, which the adversarial pass of 2026-09-05 covered and which
  this session's other four reviews cover from three other angles.
* Concurrency inside `sweep()`: unchanged this session.
* What happens when GitHub delays or drops a scheduled run, which is
  `tests/test_run_safety.py`'s subject rather than an attack.
* A scheduled run's behaviour when the corpus **shrinks** below a slice
  boundary. The arithmetic is the same as growing past one and the bound is
  asserted in one direction only.
