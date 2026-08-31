"""Parsing and normalization of untrusted tracker URLs.

the normalization contract in src/trackers/normalize.py: "Normalize only transformations proven semantically safe. Each
normalization rule MUST have a test asserting it preserves tracker identity,
and a documented reason." And: "MUST NOT normalize away meaningful tracker
identity. When in doubt, keep both forms and let deduplication decide with
evidence."

So every rule below is a named entry in `RULES` with the reason it is safe, and
`tests.test_p1.TestNormalizationRules` asserts one property per rule. A rule
with no test is a rule nobody has checked.

The bias throughout is **conservative**: a normalization that merges two
distinct trackers destroys data silently, while a normalization not performed
merely leaves a duplicate for `dedup.py` to consider with evidence. Those costs are
not symmetric.

Everything here treats its input as hostile (RULES 5.1). It parses
and rejects; it never executes, never resolves, and never derives a filesystem
path from source content.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from urllib.parse import quote, unquote, urlsplit

from .model import InvalidTracker, Network, Tracker, Transport, classify_network

#: Hard ceiling on a single candidate line. A source that sends a megabyte on
#: one line is not sending a tracker URL (RULES 5.2: bounded everything).
MAX_URL_LENGTH = 2048

#: Control characters and whitespace that must never survive into an emitted
#: URL. Includes the ones that make a plaintext line ambiguous to a consumer.
_FORBIDDEN_CHARS = re.compile(r"[\x00-\x20\x7f-\x9f]")

#: Every character RFC 3986 permits anywhere in a URI: unreserved, reserved
#: (gen-delims and sub-delims), and the percent of an escape.
#:
#: A character outside this set means the upstream shipped something that is
#: not a URI. **Measured, 2026-08-31:** three of the then-1337 published lines carried
#: one -- a stray `"` that is an HTML attribute terminator leaked by somebody's
#: scraper, and two `authkey=...|...|...` query strings. All three reached the
#: published plaintext, which is the format this project tells consumers to
#: `curl | client`, and `"` and `|` are both shell-significant in exactly that
#: idiom.
#:
#: Rejecting is the conservative direction here, unusually. The module's normal
#: bias is that not-normalizing is safer than normalizing, because merging two
#: trackers destroys data; but this is not a merge, it is a refusal to publish
#: a string that is not a URL, and the rejection is returned with its reason so
#: a consumer can see what disappeared and why (RULES 3.10). The alternative --
#: percent-encoding the offender -- would change the identity of somebody's
#: endpoint on our guess about what they meant.
_URI_ALLOWED = re.compile(r"^[A-Za-z0-9\-._~:/?#\[\]@!$&'()*+,;=%]+$")

_HOSTNAME_OK = re.compile(r"^[a-z0-9]([a-z0-9\-_]{0,62}[a-z0-9])?"
                          r"(\.[a-z0-9]([a-z0-9\-_]{0,62}[a-z0-9])?)*$")


@dataclass(frozen=True, slots=True)
class Rule:
    name: str
    why_safe: str


#: The complete set of transformations applied. Documented here so the list is
#: reviewable in one place rather than inferred from code.
RULES: tuple[Rule, ...] = (
    Rule("strip_surrounding_whitespace",
         "Leading/trailing whitespace is never part of a URL. Confirmed safe "
         "by a real consumer doing the same: torrent_miscellaneous.pas:174 "
         "calls UTF8Trim on every line before validating it."),
    Rule("strip_trailing_comment",
         "A ` # reason` suffix is upstream annotation, not URL. ngosang's "
         "blacklist.txt uses exactly this form, and the same consumer strips "
         "it by truncating at the first space. Only stripped when preceded by "
         "whitespace, so a '#' inside a path or query is preserved."),
    Rule("lowercase_scheme",
         "Schemes are case-insensitive (RFC 3986 section 3.1). UDP:// and udp:// are "
         "the same endpoint."),
    Rule("lowercase_host",
         "Hostnames are case-insensitive (RFC 4343). Does NOT touch the path, "
         "which is case-SENSITIVE and where trackers put announce keys."),
    Rule("strip_trailing_dot",
         "'example.com.' and 'example.com' resolve identically; the root dot "
         "is a DNS notation detail, not a distinct host."),
    Rule("normalize_ipv6_brackets",
         "Bracket syntax is transport notation (RFC 3986 section 3.2.2), so "
         "'[::1]:80' and the address it wraps are one host."),
    Rule("keep_explicit_port",
         "NOT a normalization -- an explicit refusal to perform one. UDP has "
         "no default-port convention, so udp://x:80/announce and "
         "udp://x/announce are DIFFERENT endpoints and merging them would "
         "invent a tracker. Ports are preserved exactly as written, including "
         "http on 80 and https on 443, because dropping them would make the "
         "canonical form disagree with what upstreams publish."),
    Rule("preserve_path_case_and_trailing_slash",
         "NOT a normalization. '/announce' and '/announce/' can be routed "
         "differently by a tracker, and announce paths carry passkeys whose "
         "case is significant. Kept verbatim; deduplication may still decide "
         "two forms are one tracker, but only with evidence."),
    Rule("decode_unreserved_percent_escapes",
         "RFC 3986 section 6.2.2.2: percent-encoded unreserved characters are "
         "equivalent to their decoded form. Only unreserved characters are "
         "decoded; reserved ones are left alone because decoding those WOULD "
         "change meaning."),
    Rule("refuse_non_uri_characters",
         "NOT a normalization -- a refusal to publish a string that is not a "
         "URL. RFC 3986 defines the character set a URI may use; anything "
         "outside it means the upstream shipped an artefact. Measured: three "
         "of the then-1337 published lines carried one, a stray '\"' leaked by an "
         "HTML "
         "scraper and two '|' in query strings, and both characters are "
         "shell-significant in the `curl | client` idiom this project's own "
         "README recommends. Rejected rather than percent-encoded, because "
         "encoding would change somebody's endpoint on our guess about what "
         "they meant; the rejection is returned with its reason so the "
         "disappearance is explainable."),
    Rule("refuse_unicode_hostnames",
         "NOT a normalization -- an explicit refusal to perform one. A "
         "punycode A-label host (`xn--e1afmkfd.xn--p1ai`) is accepted and "
         "lowercased like any other; a Unicode U-label host is REJECTED with "
         "a reason rather than encoded here. IDNA is version-dependent -- "
         "IDNA2003 and IDNA2008 disagree on real characters, notably the "
         "German sharp s and the final sigma -- so encoding would mean "
         "GUESSING which host an upstream meant, and a wrong A-label is a "
         "silently different server. A rejection is auditable and a consumer "
         "can see it (RULES 3.10); a mis-encoded host is a tracker we "
         "invented. No U-label appears anywhere in the current corpus, so "
         "this costs nothing measured today."),
)


def _strip_comment(line: str) -> str:
    """Remove a trailing ` # ...` annotation.

    Requires whitespace before the '#'. A bare '#' with no preceding space may
    be a fragment or part of a path, and removing it would silently truncate a
    real URL -- the failure this project is least willing to accept.
    """
    m = re.search(r"\s#", line)
    return line[:m.start()] if m else line


def _decode_unreserved(text: str) -> str:
    """Decode only RFC 3986 unreserved percent-escapes; leave reserved alone."""
    def repl(m: re.Match[str]) -> str:
        ch = unquote(m.group(0))
        return ch if re.fullmatch(r"[A-Za-z0-9\-._~]", ch) else m.group(0)
    return re.sub(r"%[0-9A-Fa-f]{2}", repl, text)


def parse(raw: str) -> Tracker:
    """Parse one untrusted line into a canonical `Tracker`.

    Raises `InvalidTracker` with a reason. Every rejection is explainable,
    because RULES 3.10 requires every accept/reject decision to be auditable
    after the fact.
    """
    if not isinstance(raw, str):
        raise InvalidTracker(f"not a string: {type(raw).__name__}")

    line = raw.strip()                       # strip_surrounding_whitespace
    line = _strip_comment(line).strip()      # strip_trailing_comment
    if not line:
        raise InvalidTracker("empty after stripping whitespace and comments")
    if len(line) > MAX_URL_LENGTH:
        raise InvalidTracker(f"longer than {MAX_URL_LENGTH} bytes ({len(line)})")
    # Structural check first, so the common case -- a line of prose -- gets the
    # informative reason rather than "contains whitespace". Rejections have to
    # be explainable (RULES 3.10), and "no scheme separator" tells a
    # maintainer what changed upstream while a character-class complaint does
    # not. The character check still runs, immediately below.
    if "://" not in line:
        raise InvalidTracker("no scheme separator '://'")
    if _FORBIDDEN_CHARS.search(line):
        raise InvalidTracker("contains control or whitespace characters")
    if not _URI_ALLOWED.match(line):
        bad = sorted({c for c in line if not _URI_ALLOWED.match(c)})
        raise InvalidTracker(
            "contains characters no URI may hold: "
            + " ".join(repr(c) for c in bad[:5]))

    try:
        parts = urlsplit(line)
    except ValueError as e:
        raise InvalidTracker(f"unparseable: {e}") from e

    scheme = parts.scheme.lower()            # lowercase_scheme
    try:
        transport = Transport(scheme)
    except ValueError:
        raise InvalidTracker(f"unknown transport {scheme!r}") from None

    try:
        host_raw = parts.hostname
    except ValueError as e:
        raise InvalidTracker(f"unparseable host: {e}") from e
    if not host_raw:
        raise InvalidTracker("no host")

    host = host_raw.lower().rstrip(".")      # lowercase_host, strip_trailing_dot
    if not host:
        raise InvalidTracker("host was only dots")

    # normalize_ipv6_brackets: urlsplit already removes them; keep the bare
    # address as the canonical host and re-add brackets only when rendering.
    if ":" in host:
        try:
            import ipaddress
            ipaddress.ip_address(host)
        except ValueError:
            raise InvalidTracker(f"host {host!r} contains ':' but is not an IP") from None
    elif not _HOSTNAME_OK.match(host):
        # Reject hostnames that could not be a DNS name. This is what keeps a
        # source-supplied string from ever looking like a path or a shell word.
        raise InvalidTracker(f"host {host!r} is not a valid hostname or IP")

    try:
        port = parts.port                    # keep_explicit_port
    except ValueError as e:
        raise InvalidTracker(f"invalid port: {e}") from e
    if port is not None and not (0 < port < 65536):
        raise InvalidTracker(f"port out of range: {port}")

    path = _decode_unreserved(parts.path)    # decode_unreserved_percent_escapes
    query = _decode_unreserved(parts.query)

    network = classify_network(host)
    netloc = f"[{host}]" if ":" in host else host
    if port is not None:
        netloc = f"{netloc}:{port}"
    url = f"{transport.value}://{netloc}{path}" + (f"?{query}" if query else "")

    return Tracker(url=url, transport=transport, network=network, host=host,
                   port=port, path=path, query=query)


def parse_many(lines) -> tuple[list[Tracker], list[tuple[str, str]]]:
    """Parse many candidates. Returns (accepted, [(raw, reason), ...]).

    Rejections are *returned*, never dropped: a tracker vanishing from the
    dataset must be explainable to the consumer who noticed (RULES 3.10; T-066
    owns the report). Returning them also means a caller cannot accidentally ignore them
    the way a logged warning can be ignored.
    """
    accepted: list[Tracker] = []
    rejected: list[tuple[str, str]] = []
    for raw in lines:
        text = raw if isinstance(raw, str) else str(raw)
        stripped = text.strip()
        # A blank line or a whole-line comment is not a rejection; it is
        # ordinary list formatting. Counting it as one would make every source
        # look broken. newTrackon's own /api/live is blank-line separated:
        # measured 156 lines, 78 non-blank, 78 blank.
        if not stripped or stripped.startswith("#"):
            continue
        try:
            accepted.append(parse(text))
        except InvalidTracker as e:
            rejected.append((stripped[:MAX_URL_LENGTH], str(e)))
    return accepted, rejected
