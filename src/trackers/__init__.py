"""trackers: evidence-driven tracker aggregation.

Standard library only, by decision D1 (`HISTORY/decisions.md`), enforced by
`scripts/check-no-third-party-imports.py`.
"""

import sys

#: The supported interpreter floor, enforced rather than documented.
#:
#: RULES 12 used to say "Python 3.12+" while nothing anywhere required or
#: checked 3.12, and the whole suite passes on 3.11. A version floor nobody
#: enforces is documentation, not a constraint, and one set above what the code
#: needs excludes contributors for no measured reason (RULES 15.5).
#:
#: 3.11 is the real floor: `dataclass(slots=True)` needs 3.10, and
#: `tomllib`/`ExceptionGroup`-era syntax is not used. Raise this only when a
#: feature actually requires it, and say which in the same change.
MINIMUM_PYTHON = (3, 11)

if sys.version_info < MINIMUM_PYTHON:  # pragma: no cover - guard, not logic
    raise RuntimeError(
        "trackers needs Python "
        f"{MINIMUM_PYTHON[0]}.{MINIMUM_PYTHON[1]} or newer; this is "
        f"{sys.version_info[0]}.{sys.version_info[1]}. "
        "See TODO/RULES.md section 12."
    )

__version__ = "0.1.0"

#: Bumped when normalization or deduplication changes semantics, so a consumer
#: can tell which rules produced a dataset (T-062).
NORMALIZATION_VERSION = 1
