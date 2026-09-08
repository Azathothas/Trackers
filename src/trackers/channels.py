"""Release channels, built on what the platform was measured to do. T-064, D5.

Three channels: a rolling one updated by every successful generation, a daily
one, and a weekly one. What each may be implemented as is not a design choice
here -- `experiments/24-release-channel-behaviour.py` measured the platform on
2026-09-05 and three of its answers decide this module's shape.

⛔ **A TAG NAMED `latest` EARNS NOTHING, SO THE ROLLING CHANNEL IS NOT CALLED
THAT** (`C-14`). GitHub's `/releases/latest` resolves to the newest
non-prerelease **by date**, and a release tagged `latest` created first lost
that endpoint to a newer one tagged `test-t003-newer`. So the name is a
coincidence rather than a contract, and D5's own instruction is to rename the
channel rather than ship one. It is `rolling`.

⛔ **A MOVED TAG DOES NOT MOVE THE RELEASE** (`C-17`, refuted as originally
claimed). The tag ref moved and `target_commitish` stayed at the old commit, so
a consumer reading the release object and one reading the tag get different
answers and neither is wrong. Updating a channel is therefore
**delete-and-recreate**, never a tag move.

⛔ **THERE IS NO READ-AFTER-WRITE** (`C-15`). An asset can be replaced at a
stable URL, and that URL was measured serving the **previous bytes 10 seconds
later** with the old `ETag` and no `Cache-Control` at all. So a publication
step that replaces an asset and then fetches it back to confirm is asserting
something the platform does not promise: verification reads the API's asset
metadata, never the download URL.

⛔ **A FAILED OR SUSPICIOUS RUN NEVER TAKES A CHANNEL.** That is D5's own
wording and it is the only rule here that is about our data rather than about
the platform. `promotable` is where it lives, and it is a returned value with
a reason (RULES 3.10) rather than a boolean nobody can audit.

THE CLOCK IS INJECTED, as everywhere else (RULES 3.6). A channel that decides
what day it is by asking the machine is a channel whose output depends on when
the test ran.
"""

from __future__ import annotations

import datetime as _dt
from dataclasses import dataclass
from enum import Enum
from typing import Any

__all__ = [
    "Channel", "ChannelDecision", "TAGS", "iso_week_tag", "daily_tag",
    "tag_for", "promotable", "due", "ROLLING_TAG",
]

#: ⛔ Not `latest`. `C-14` measured that name earning nothing from the platform
#: while implying a contract to a reader, which is exactly the naming
#: coincidence D5 says not to ship.
ROLLING_TAG = "rolling"


class Channel(str, Enum):
    """The three, and what each promises.

    `ROLLING` moves on every successful generation. `DAILY` and `WEEKLY` are
    snapshots: once per period, and a second run in the same period does not
    replace one, because a consumer pinning `daily-2026-09-08` is pinning that
    day rather than the last run of it.
    """

    ROLLING = "rolling"
    DAILY = "daily"
    WEEKLY = "weekly"


#: The tag each channel publishes under. The rolling one is fixed; the others
#: carry their period, so a consumer can pin one and it never moves.
TAGS: dict[Channel, str] = {
    Channel.ROLLING: ROLLING_TAG,
    Channel.DAILY: "daily-",
    Channel.WEEKLY: "weekly-",
}


@dataclass(frozen=True, slots=True)
class ChannelDecision:
    """Whether a channel is published this run, and why or why not.

    RULES 3.10: a channel that silently does not update owes the person who
    noticed a reason, so the reason is returned rather than logged.
    """

    channel: Channel
    publish: bool
    tag: str
    reason: str

    def as_record(self) -> dict[str, Any]:
        return {"channel": self.channel.value, "publish": self.publish,
                "tag": self.tag, "reason": self.reason}


def _parse(instant: str) -> _dt.datetime:
    """An ISO 8601 UTC instant, as the pipeline writes them.

    ⛔ Raises on anything else rather than guessing. A channel that silently
    accepted a malformed timestamp would publish under a tag nobody can
    predict.
    """
    text = instant.strip()
    if text.endswith("Z"):
        text = text[:-1] + "+00:00"
    parsed = _dt.datetime.fromisoformat(text)
    if parsed.tzinfo is None:
        raise ValueError(f"{instant!r} has no timezone; UTC must be explicit")
    return parsed.astimezone(_dt.timezone.utc)


def daily_tag(generated_at: str) -> str:
    """`daily-YYYY-MM-DD`, in UTC. The day is the UTC day, always."""
    return TAGS[Channel.DAILY] + _parse(generated_at).strftime("%Y-%m-%d")


def iso_week_tag(generated_at: str) -> str:
    """`weekly-YYYY-Www`, ISO 8601, so the week starts on **Monday**.

    ⭐ D5 says the convention SHOULD be ISO 8601 unless evidence favours
    otherwise and MUST be explicit. There is no evidence favouring otherwise,
    and this is the explicit part: `isocalendar` decides, so a week that
    straddles a year boundary belongs to the year ISO says it does rather than
    to whichever year the date happens to carry.
    """
    year, week, _ = _parse(generated_at).isocalendar()
    return f"{TAGS[Channel.WEEKLY]}{year}-W{week:02d}"


def tag_for(channel: Channel, generated_at: str) -> str:
    """The tag this channel publishes under for this instant."""
    if channel is Channel.ROLLING:
        return ROLLING_TAG
    if channel is Channel.DAILY:
        return daily_tag(generated_at)
    return iso_week_tag(generated_at)


def promotable(*, generation_succeeded: bool, volume_change_suspicious: bool,
               tracker_count: int) -> tuple[bool, str]:
    """Whether this run's output may take a channel at all.

    ⛔ **A failed or suspicious run never replaces a channel** (D5). The three
    inputs are separate because they fail differently: a failed generation
    produced nothing, a suspicious volume change produced something that may be
    wrong, and an empty output is the all-dead dataset RULES 11 names.
    """
    if not generation_succeeded:
        return False, "the generation failed; the previous channel stands"
    if tracker_count <= 0:
        return False, ("the generation produced no trackers, which is the "
                       "all-dead dataset the volume guard exists for")
    if volume_change_suspicious:
        return False, ("the volume change is outside the recorded range; a "
                       "channel is not moved on a suspicious run")
    return True, f"generation succeeded with {tracker_count} trackers"


def due(channel: Channel, generated_at: str,
        published_tags: set[str]) -> ChannelDecision:
    """Whether this channel is published for this instant.

    `published_tags` is what already exists on the remote. ⭐ **A daily or
    weekly tag that exists is not republished**, because a consumer pinning
    `daily-2026-09-08` pinned that day and not the last run inside it. The
    rolling channel has no such promise and is always due.

    ⚠ **Deciding this from tags rather than from a stored cursor** is
    deliberate: the remote is the thing a consumer reads, so the remote is the
    thing that says what has been published. A cursor can disagree with it.
    """
    tag = tag_for(channel, generated_at)
    if channel is Channel.ROLLING:
        return ChannelDecision(channel, True, tag,
                               "the rolling channel moves on every successful "
                               "generation")
    if tag in published_tags:
        return ChannelDecision(channel, False, tag,
                               f"{tag} already exists and a period is "
                               f"published once")
    return ChannelDecision(channel, True, tag, f"{tag} has not been published")
