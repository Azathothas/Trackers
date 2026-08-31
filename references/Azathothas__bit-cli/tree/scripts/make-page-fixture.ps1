# Build the proving ground for web page link extraction.
#
# `TODO/cli-surface.md` T-244 ships two tiers over one extractor: a static one
# that reads what the server sent, and `--render`, which reads the DOM after
# script has run. The question neither a single fixture page nor a real site
# can answer is **where the static tier stops working**, because a real site
# does not come with a known-correct answer and a single flat page proves only
# the easy case.
#
# So this emits six pages of escalating difficulty, each with the correct
# extraction written beside it as JSON. That second half is what makes it a
# fixture rather than a demo: a run compares against a recorded answer instead
# of against whatever the extractor happened to do.
#
#   L0  flat        absolute, root-relative and relative hrefs
#   L1  structure   several hundred links in deeply nested markup
#   L2  addressing  <base href>, protocol-relative, ../.., percent-encoded,
#                   uppercase .TORRENT, a query after the extension, a
#                   fragment, and a magnet carrying every field the grammar has
#   L3  decoys      .torrent in the text only, .torrent.html, a data: URI, an
#                   off-host link, one in a comment, one in <noscript>, and a
#                   duplicate of a real link
#   L4  script      document.write, innerHTML, DOMContentLoaded, setTimeout,
#                   a fetch response, and a <template> a script clones
#   L5  hostile     shadow DOM, an iframe carrying the only link, a click
#                   handler with no href, and a page that renders nothing at
#                   all without script
#   L6  hidden      a link behind a query parameter with no extension whose
#                   only label is an image's alt, a <link rel="alternate">
#                   advertisement, a data-* attribute a script reads, and the
#                   three shapes a looser rule would wrongly take
#   L7  unfriendly  links assembled from split strings, one built two frames
#                   later, one that needs a click and one that needs a scroll,
#                   plus a challenge interstitial that must be **refused**
#
# Each expectation carries **two** lists. `static` is what the static tier must
# find and `rendered` is what a browser must find. They are identical for L0 to
# L3, and any difference there is a defect in the extractor rather than a
# property of the page. L4 and L5 are where they diverge, and the size of that
# divergence is the argument for `--render` existing at all.
#
# Three smaller cases sit beside the levels for the entry's own acceptance:
# `one-torrent`, `one-magnet` and `two-of-each`.
#
# Usage:
#   pwsh scripts/make-page-fixture.ps1
#   pwsh scripts/make-page-fixture.ps1 -BaseUrl http://127.0.0.1:8099 -Level 2
#   pwsh scripts/make-page-fixture.ps1 -Seed 7 -Root .tmp/pages
#
# `-BaseUrl` is written into the absolute hrefs and into every expectation, so
# the pages are correct for the server that will serve them. `-Seed` makes the
# filler reproducible.
#
# Exits 0 when every page was written, 2 when it could not run. The fixture
# stays where it is put; nothing here removes it.
#
# See TODO/cli-surface.md, T-244.

[CmdletBinding()]
param(
    # Where the pages will be served from. Absolute hrefs and every expected
    # URL are built against this.
    [string]$BaseUrl = "http://127.0.0.1:8099",
    # One level only. Omitted, every level and every acceptance case.
    [ValidateRange(-1, 7)]
    [int]$Level = -1,
    [int]$Seed = 20260829,
    [string]$Root = ".tmp/page-fixture",
    # Report the manifest as JSON on stdout as well as writing it.
    [switch]$Json
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("make-page-fixture: $message")
    exit $code
}

$BaseUrl = $BaseUrl.TrimEnd('/')
if ($BaseUrl -notmatch '^https?://') { Exit-With 2 "-BaseUrl must be an http:// or https:// URL" }

$outRoot = if ([System.IO.Path]::IsPathRooted($Root)) { $Root } else { Join-Path $repo $Root }
try { New-Item -ItemType Directory -Force -Path $outRoot | Out-Null }
catch { Exit-With 2 "cannot create $outRoot : $($_.Exception.Message)" }

# The ANSI C linear congruential generator, bits 16 to 23, which is what
# scripts/make-scenario-fixture.ps1 already uses for reproducible filler.
$script:lcg = [uint32]$Seed
function Next-Byte {
    $script:lcg = [uint32](($script:lcg * 1103515245 + 12345) % 2147483648)
    return [int](($script:lcg -shr 16) -band 0xFF)
}
function Next-Int([int]$max) { return (Next-Byte) % $max }

$words = @('alpha', 'bravo', 'delta', 'echo', 'kilo', 'lima', 'nova', 'oscar', 'romeo', 'sierra',
    'tango', 'ultra', 'victor', 'whisky', 'xray', 'yankee', 'zulu', 'atlas', 'basalt', 'cobalt')
function Next-Word { return $words[(Next-Int $words.Count)] }

# A well-formed magnet with every field the URI grammar allows, so L2 proves
# the whole thing survives extraction rather than only `xt`.
$fullMagnet = 'magnet:?xt=urn:btih:9e20e33071fae16fc950cd95e5fc6ec0059d9a63' +
'&dn=Example+Payload+24.04' +
'&xl=1758298112' +
'&tr=udp%3A%2F%2Ftracker.example%3A6969%2Fannounce' +
'&tr=http%3A%2F%2Ftracker2.example%3A80%2Fannounce' +
'&ws=https%3A%2F%2Fmirror.example%2Fpayload%2F' +
'&as=https%3A%2F%2Falt.example%2Fpayload.iso' +
'&kt=example+payload+iso' +
'&so=0-2%2C4' +
'&x.pe=192.0.2.11%3A6881'
$shortMagnet = 'magnet:?xt=urn:btih:0102030405060708090a0b0c0d0e0f1011121314&dn=Short+One'

# One expected link. `url` is already absolute, the way the extractor reports
# it, so a comparison is a string comparison and not a second resolver.
function New-Expect([string]$url, [string]$text, [string]$kind) {
    return [ordered]@{ url = $url; text = $text; kind = $kind }
}

$cases = @()

# ---------------------------------------------------------------------------
# L0 flat
# ---------------------------------------------------------------------------
function Build-L0 {
    $html = @"
<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>L0 flat</title></head>
<body>
<h1>Downloads</h1>
<ul>
  <li><a href="$BaseUrl/files/one.torrent">Example 24.04 Desktop amd64</a></li>
  <li><a href="/files/two.torrent">Example 24.04 Server amd64</a></li>
  <li><a href="files/three.torrent">Example 24.04 Live arm64</a></li>
  <li><a href="$shortMagnet">Example 24.04 (magnet)</a></li>
  <li><a href="/files/checksums.txt">SHA256SUMS</a></li>
  <li><a href="https://example.org/about">About this mirror</a></li>
</ul>
</body></html>
"@
    $expect = @(
        (New-Expect "$BaseUrl/files/one.torrent" "Example 24.04 Desktop amd64" "torrent")
        (New-Expect "$BaseUrl/files/two.torrent" "Example 24.04 Server amd64" "torrent")
        (New-Expect "$BaseUrl/files/three.torrent" "Example 24.04 Live arm64" "torrent")
        (New-Expect $shortMagnet "Example 24.04 (magnet)" "magnet")
    )
    return @{ name = "L0-flat"; level = 0; html = $html; static = $expect; rendered = $expect }
}

# ---------------------------------------------------------------------------
# L1 structure: the same links, buried
# ---------------------------------------------------------------------------
function Build-L1 {
    $sb = [System.Text.StringBuilder]::new()
    [void]$sb.AppendLine('<!doctype html>')
    [void]$sb.AppendLine('<html lang="en"><head><meta charset="utf-8"><title>L1 structure</title></head><body>')
    $expect = @()
    $realAt = @(37, 118, 204, 291, 388)
    $realIndex = 0
    for ($i = 0; $i -lt 420; $i++) {
        $depth = 1 + (Next-Int 6)
        $open = ""
        $close = ""
        for ($d = 0; $d -lt $depth; $d++) {
            $tag = @('div', 'table><tr><td', 'ul><li', 'section', 'span')[(Next-Int 5)]
            $open += "<$tag>"
            $closeTag = switch ($tag) {
                'table><tr><td' { '</td></tr></table>' }
                'ul><li' { '</li></ul>' }
                default { "</$tag>" }
            }
            $close = $closeTag + $close
        }
        if ($realAt -contains $i) {
            $realIndex++
            $name = "release-$realIndex"
            $text = "Example $realIndex.0 $(Next-Word)"
            [void]$sb.AppendLine("$open<a href=`"/deep/$name.torrent`">$text</a>$close")
            $expect += (New-Expect "$BaseUrl/deep/$name.torrent" $text "torrent")
        } else {
            $w = Next-Word
            [void]$sb.AppendLine("$open<a href=`"/page/$w-$i.html`">$w $i</a>$close")
        }
    }
    [void]$sb.AppendLine('</body></html>')
    return @{ name = "L1-structure"; level = 1; html = $sb.ToString(); static = $expect; rendered = $expect }
}

# ---------------------------------------------------------------------------
# L2 addressing
# ---------------------------------------------------------------------------
function Build-L2 {
    # The document is served from /nested/deep/ and the base sends every
    # relative href somewhere else, which is the whole point of the level.
    $html = @"
<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<base href="$BaseUrl/files/">
<title>L2 addressing</title></head>
<body>
<a href="rel.torrent">Relative under the base</a>
<a href="../up.torrent">One level up from the base</a>
<a href="//127.0.0.1:$($BaseUrl.Split(':')[-1])/proto-relative.torrent">Protocol relative</a>
<a href="/pct%20space/encoded.torrent">Percent encoded path</a>
<a href="/pct/dotted%2Etorrent">Percent encoded extension</a>
<a href="/UPPER.TORRENT">Uppercase extension</a>
<a href="/query.torrent?download=1&amp;id=7">Query after the extension</a>
<a href="/fragment.torrent#section">Fragment after the extension</a>
<a href="$fullMagnet">Every magnet field</a>
</body></html>
"@
    $port = $BaseUrl.Split(':')[-1]
    $expect = @(
        (New-Expect "$BaseUrl/files/rel.torrent" "Relative under the base" "torrent")
        (New-Expect "$BaseUrl/up.torrent" "One level up from the base" "torrent")
        (New-Expect "http://127.0.0.1:$port/proto-relative.torrent" "Protocol relative" "torrent")
        (New-Expect "$BaseUrl/pct%20space/encoded.torrent" "Percent encoded path" "torrent")
        (New-Expect "$BaseUrl/pct/dotted%2Etorrent" "Percent encoded extension" "torrent")
        (New-Expect "$BaseUrl/UPPER.TORRENT" "Uppercase extension" "torrent")
        (New-Expect "$BaseUrl/query.torrent?download=1&id=7" "Query after the extension" "torrent")
        (New-Expect "$BaseUrl/fragment.torrent#section" "Fragment after the extension" "torrent")
        (New-Expect $fullMagnet "Every magnet field" "magnet")
    )
    return @{ name = "L2-addressing"; level = 2; html = $html; static = $expect; rendered = $expect }
}

# ---------------------------------------------------------------------------
# L3 decoys
# ---------------------------------------------------------------------------
function Build-L3 {
    $html = @"
<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>L3 decoys</title></head>
<body>
<p><a href="/downloads/">grab the example-24.04.torrent from here</a></p>
<p><a href="/notes/example.torrent.html">example.torrent.html, a page about a torrent</a></p>
<p><a href="data:application/x-bittorrent;base64,ZDg6YW5ub3VuY2UzNjpo">inline data URI</a></p>
<p><a href="https://mirror.example.net/off-host.torrent">On another host, and still real</a></p>
<!-- <a href="/commented-out.torrent">Commented out</a> -->
<noscript><a href="/noscript-only.torrent">Only without script</a></noscript>
<p><a href="/files/real.torrent">The real one</a></p>
<p><a href="/files/real.torrent">The real one again</a></p>
<script>var s = '<a href="/inside-script.torrent">in a script string</a>';</script>
<style>/* <a href="/inside-style.torrent">in a style comment</a> */</style>
</body></html>
"@
    # The off-host link IS a match. Measured: kali.org serves its download page
    # from www.kali.org and every one of its 113 torrent links sits on
    # cdimage.kali.org, so a same-host rule returns nothing there.
    $expect = @(
        (New-Expect "https://mirror.example.net/off-host.torrent" "On another host, and still real" "torrent")
        (New-Expect "$BaseUrl/files/real.torrent" "The real one" "torrent")
    )
    return @{ name = "L3-decoys"; level = 3; html = $html; static = $expect; rendered = $expect }
}

# ---------------------------------------------------------------------------
# L4 script: six ways to build a link that is not in the source
# ---------------------------------------------------------------------------
function Build-L4 {
    $html = @"
<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>L4 script</title></head>
<body>
<p><a href="/files/in-source.torrent">Present in the source</a></p>
<div id="written"></div>
<div id="inner"></div>
<div id="onload"></div>
<div id="later"></div>
<div id="fetched"></div>
<div id="cloned"></div>
<template id="tpl"><a href="/files/from-template.torrent">Cloned from a template</a></template>
<script>
document.write('<a href="/files/document-write.torrent">Written by document.write</a>');
document.getElementById('inner').innerHTML =
  '<a href="/files/inner-html.torrent">Assigned to innerHTML</a>';
document.addEventListener('DOMContentLoaded', function () {
  var a = document.createElement('a');
  a.href = '/files/on-ready.torrent';
  a.textContent = 'Built on DOMContentLoaded';
  document.getElementById('onload').appendChild(a);
  document.getElementById('cloned').appendChild(
    document.getElementById('tpl').content.cloneNode(true));
});
setTimeout(function () {
  document.getElementById('later').innerHTML =
    '<a href="/files/after-timeout.torrent">Built after a timeout</a>';
}, 50);
fetch('links.json').then(function (r) { return r.json(); }).then(function (d) {
  document.getElementById('fetched').innerHTML =
    '<a href="' + d.href + '">' + d.text + '</a>';
}).catch(function () {});
</script>
</body></html>
"@
    $static = @(
        (New-Expect "$BaseUrl/files/in-source.torrent" "Present in the source" "torrent")
    )
    # The order is **measured** and not predicted. Six of these links are put
    # in the document by script, and where each lands is the browser's
    # decision rather than the order the script that made it appears in. The
    # first version of this list was written by reading the markup top to
    # bottom and it was wrong in two places: `document.write` after the parser
    # has passed the point ends up last, and a clone of a `<template>` lands
    # where its container is rather than where the template is. Chrome 151,
    # 2026-08-29, through `--render`.
    $rendered = @(
        (New-Expect "$BaseUrl/files/in-source.torrent" "Present in the source" "torrent")
        (New-Expect "$BaseUrl/files/inner-html.torrent" "Assigned to innerHTML" "torrent")
        (New-Expect "$BaseUrl/files/on-ready.torrent" "Built on DOMContentLoaded" "torrent")
        (New-Expect "$BaseUrl/files/after-timeout.torrent" "Built after a timeout" "torrent")
        (New-Expect "$BaseUrl/files/from-fetch.torrent" "Built from a fetch response" "torrent")
        (New-Expect "$BaseUrl/files/from-template.torrent" "Cloned from a template" "torrent")
        (New-Expect "$BaseUrl/files/document-write.torrent" "Written by document.write" "torrent")
    )
    return @{
        name     = "L4-script"; level = 4; html = $html; static = $static; rendered = $rendered
        sidecars = @{ "links.json" = '{"href":"/files/from-fetch.torrent","text":"Built from a fetch response"}' }
    }
}

# ---------------------------------------------------------------------------
# L5 hostile: not adversarial, just built the way a modern app is built
# ---------------------------------------------------------------------------
function Build-L5 {
    $frame = @"
<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>L5 frame</title></head>
<body><a href="/files/in-iframe.torrent">Inside an iframe</a></body></html>
"@
    $html = @"
<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>L5 hostile</title></head>
<body>
<div id="root"></div>
<div id="host"></div>
<iframe src="frame.html" width="400" height="80" title="downloads"></iframe>
<button id="go" data-href="/files/behind-a-handler.torrent">Download the torrent</button>
<script>
document.getElementById('root').innerHTML =
  '<h1>Downloads</h1><p><a href="/files/rendered-only.torrent">Only after script runs</a></p>';
var host = document.getElementById('host').attachShadow({ mode: 'open' });
host.innerHTML = '<a href="/files/in-shadow-dom.torrent">Inside a shadow root</a>';
document.getElementById('go').addEventListener('click', function () {
  location.href = this.dataset.href;
});
</script>
</body></html>
"@
    # Nothing is expected from the static tier: the page's whole body is built
    # by script. The click handler is expected from neither tier, because a
    # button with no href is not a link in either one.
    $rendered = @(
        (New-Expect "$BaseUrl/files/rendered-only.torrent" "Only after script runs" "torrent")
        (New-Expect "$BaseUrl/files/in-shadow-dom.torrent" "Inside a shadow root" "torrent")
    )
    return @{
        name     = "L5-hostile"; level = 5; html = $html; static = @(); rendered = $rendered
        sidecars = @{ "frame.html" = $frame }
        notes    = "the iframe's link is in a second document, which neither tier reads from the parent; the button has no href and is a link in neither"
    }
}

# ---------------------------------------------------------------------------
# L6 hidden addressing: the link is there and nothing about the URL says so
# ---------------------------------------------------------------------------
#
# Every shape here was measured on a real page rather than imagined.
# `linuxtracker.org`, one of the fifteen `scripts/check-page-fetch.ps1`
# fetches, publishes every torrent behind `index.php?page=downloadcheck&id=`
# with **no anchor text at all**: the link wraps an icon and the label is the
# image's `alt`. That page yields nothing to an extension rule.
function Build-L6 {
    $hex = "1608515a36c7e233742c42daf54d39f05a5f9aeb"
    $html = @"
<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>L6 hidden addressing</title>
<link rel="alternate" type="application/x-bittorrent" href="/feed/latest?rel=24.04" title="Latest release">
<link rel="stylesheet" href="/site.css">
</head>
<body>
<h1>Downloads</h1>
<p><a href="/files/plain.torrent">Example 24.04 Desktop</a></p>
<p><a class="lasttor" href="index.php?page=downloadcheck&amp;id=$hex"><img src="images/torrent.gif" border="0" alt="Download Torrent" title="Download Torrent" /></a></p>
<p><a href="index.php?page=torrents">Torrents</a></p>
<p><a href="/browse">Download torrent</a></p>
<p><a href="/get?ref=">Download torrent</a></p>
<p><a href="/opaque?id=99">Mirror three</a></p>
<div id="late" data-href="/files/from-data.torrent" data-text="Read out of a data attribute"></div>
<script>
var slot = document.getElementById('late');
slot.innerHTML = '<a href="' + slot.dataset.href + '">' + slot.dataset.text + '</a>';
</script>
</body></html>
"@
    # Three of the six anchors and one of the two `<link>`s are torrents, and
    # each is taken by a different rule. The three that are not are the ones a
    # loose rule would take: a navigation link with the same label shape, a
    # torrent label over a path with no identifier, and an opaque link with an
    # identifier and no label.
    $static = @(
        (New-Expect "$BaseUrl/feed/latest?rel=24.04" "Latest release" "torrent")
        (New-Expect "$BaseUrl/files/plain.torrent" "Example 24.04 Desktop" "torrent")
        (New-Expect "$BaseUrl/index.php?page=downloadcheck&id=$hex" "Download Torrent" "torrent")
    )
    $rendered = @(
        (New-Expect "$BaseUrl/feed/latest?rel=24.04" "Latest release" "torrent")
        (New-Expect "$BaseUrl/files/plain.torrent" "Example 24.04 Desktop" "torrent")
        (New-Expect "$BaseUrl/index.php?page=downloadcheck&id=$hex" "Download Torrent" "torrent")
        (New-Expect "$BaseUrl/files/from-data.torrent" "Read out of a data attribute" "torrent")
    )
    return @{
        name     = "L6-hidden"; level = 6; html = $html; static = $static; rendered = $rendered
        notes    = "the data-* link needs a browser; the navigation link, the labelled path with no identifier and the opaque identifier with no label are the three a looser rule would wrongly take"
    }
}

# ---------------------------------------------------------------------------
# L7 unfriendly: a page that does not want to be read by a program
# ---------------------------------------------------------------------------
function Build-L7 {
    $html = @"
<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>L7 unfriendly</title></head>
<body>
<div id="assembled"></div>
<div id="deferred"></div>
<button id="reveal" data-href="/files/behind-a-click.torrent">Show the download</button>
<div id="onscroll"></div>
<script>
var a = ['/fi', 'les/', 'assem', 'bled.', 'torr', 'ent'];
document.getElementById('assembled').innerHTML =
  '<a href="' + a.join('') + '">Assembled from pieces</a>';
requestAnimationFrame(function () {
  setTimeout(function () {
    document.getElementById('deferred').innerHTML =
      '<a href="/files/two-frames-later.torrent">Two frames later</a>';
  }, 120);
});
document.getElementById('reveal').addEventListener('click', function () {
  document.body.insertAdjacentHTML('beforeend',
    '<a href="' + this.dataset.href + '">Only after a click</a>');
});
window.addEventListener('scroll', function () {
  document.getElementById('onscroll').innerHTML =
    '<a href="/files/after-a-scroll.torrent">Only after a scroll</a>';
});
</script>
</body></html>
"@
    # Nothing static: every link is built by script. The click and the scroll
    # are expected from **neither** tier, and that is the point of having
    # them: `--render` loads a page, it does not use it. A tier that clicked
    # things would be a tier that submits forms.
    $rendered = @(
        (New-Expect "$BaseUrl/files/assembled.torrent" "Assembled from pieces" "torrent")
        (New-Expect "$BaseUrl/files/two-frames-later.torrent" "Two frames later" "torrent")
    )
    return @{
        name     = "L7-unfriendly"; level = 7; html = $html; static = @(); rendered = $rendered
        notes    = "the click and the scroll links are expected from neither tier: rendering loads a page, it does not drive one"
    }
}

# ---------------------------------------------------------------------------
# L7's other half: a challenge, which is refused and never solved
# ---------------------------------------------------------------------------
function Build-L7Challenge {
    $html = @"
<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>Just a moment...</title>
<meta http-equiv="refresh" content="4;url=/L7-challenge.html">
</head>
<body>
<div class="cf-browser-verification">
<h1>Checking your browser before accessing this site</h1>
<p id="cf-spinner-please-wait">This process is automatic. Your browser will redirect to your requested content shortly.</p>
<form id="challenge-form" action="/cdn-cgi/l/chk_jschl" method="POST">
<input type="hidden" name="r" value="0f2c9f0e2b" />
<input type="hidden" name="jschl_vc" value="7a1c0b1a" />
</form>
<a href="/files/definitely-not-a-torrent.html">Continue</a>
</div>
<script src="/cdn-cgi/challenge-platform/scripts/jsd/main.js"></script>
</body></html>
"@
    # Nothing from either tier, and that is the expected answer. The page
    # carries no torrent link, so `bit-cli` refuses it and says so. Nothing
    # here is solved, submitted or retried: the form is present precisely so
    # that a future change which starts posting one fails this case.
    return @{
        name     = "L7-challenge"; level = 7; html = $html; static = @(); rendered = @()
        notes    = "a challenge is refused rather than solved: no form is posted, no meta refresh is followed, and nothing is retried"
    }
}

# ---------------------------------------------------------------------------
# The three acceptance cases
# ---------------------------------------------------------------------------
function Build-Acceptance {
    $one = @"
<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>One torrent</title></head>
<body><p><a href="/files/only.torrent">The only torrent here</a></p>
<p><a href="/files/notes.txt">Release notes</a></p></body></html>
"@
    $mag = @"
<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>One magnet</title></head>
<body><p><a href="$shortMagnet">The only magnet here</a></p></body></html>
"@
    $both = @"
<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>One of each</title></head>
<body><p><a href="/files/only.torrent">Example 24.04 torrent</a></p>
<p><a href="$shortMagnet">Example 24.04 magnet</a></p></body></html>
"@
    $two = @"
<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>Two of each</title></head>
<body>
<p><a href="/files/first.torrent">Example 24.04 Desktop</a></p>
<p><a href="/files/second.torrent">Example 24.04 Server</a></p>
<p><a href="$shortMagnet">Example 24.04 Desktop magnet</a></p>
<p><a href="$fullMagnet">Example 24.04 Server magnet</a></p>
</body></html>
"@
    return @(
        @{
            name   = "one-torrent"; level = 0; html = $one
            static = @((New-Expect "$BaseUrl/files/only.torrent" "The only torrent here" "torrent"))
        }
        @{
            name   = "one-magnet"; level = 0; html = $mag
            static = @((New-Expect $shortMagnet "The only magnet here" "magnet"))
        }
        @{
            name   = "one-of-each"; level = 0; html = $both
            static = @(
                (New-Expect "$BaseUrl/files/only.torrent" "Example 24.04 torrent" "torrent")
                (New-Expect $shortMagnet "Example 24.04 magnet" "magnet")
            )
        }
        @{
            name   = "two-of-each"; level = 0; html = $two
            static = @(
                (New-Expect "$BaseUrl/files/first.torrent" "Example 24.04 Desktop" "torrent")
                (New-Expect "$BaseUrl/files/second.torrent" "Example 24.04 Server" "torrent")
                (New-Expect $shortMagnet "Example 24.04 Desktop magnet" "magnet")
                (New-Expect $fullMagnet "Example 24.04 Server magnet" "magnet")
            )
        }
    )
}

$builders = @(
    @{ level = 0; build = { Build-L0 } }
    @{ level = 1; build = { Build-L1 } }
    @{ level = 2; build = { Build-L2 } }
    @{ level = 3; build = { Build-L3 } }
    @{ level = 4; build = { Build-L4 } }
    @{ level = 5; build = { Build-L5 } }
    @{ level = 6; build = { Build-L6 } }
    @{ level = 7; build = { Build-L7 } }
    @{ level = 7; build = { Build-L7Challenge } }
)

foreach ($b in $builders) {
    if ($Level -ge 0 -and $b.level -ne $Level) { continue }
    $cases += (& $b.build)
}
if ($Level -lt 0) { $cases += Build-Acceptance }

if ($cases.Count -eq 0) { Exit-With 2 "no cases selected" }

$manifest = @()
foreach ($case in $cases) {
    $page = "$($case.name).html"
    [System.IO.File]::WriteAllText((Join-Path $outRoot $page), $case.html)
    foreach ($name in $case.sidecars.Keys) {
        [System.IO.File]::WriteAllText((Join-Path $outRoot $name), $case.sidecars[$name])
    }
    $rendered = if ($null -ne $case.rendered) { $case.rendered } else { $case.static }
    $expected = [ordered]@{
        case     = $case.name
        level    = $case.level
        base_url = $BaseUrl
        page     = $page
        url      = "$BaseUrl/$page"
        notes    = $case.notes
        static   = @($case.static)
        rendered = @($rendered)
    }
    $expectedFile = "$($case.name).expected.json"
    [System.IO.File]::WriteAllText(
        (Join-Path $outRoot $expectedFile),
        ($expected | ConvertTo-Json -Depth 6))
    $manifest += [ordered]@{
        case          = $case.name
        level         = $case.level
        page          = $page
        expected      = $expectedFile
        url           = "$BaseUrl/$page"
        static_count  = @($case.static).Count
        rendered_count = @($rendered).Count
    }
    Write-Host ("{0,-14} level {1}  {2,3} static  {3,3} rendered  {4}" -f `
            $case.name, $case.level, @($case.static).Count, @($rendered).Count, $page)
}

$index = [ordered]@{
    schema    = "page-fixture/1"
    generated = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
    base_url  = $BaseUrl
    seed      = $Seed
    root      = $outRoot
    cases     = $manifest
}
$indexText = $index | ConvertTo-Json -Depth 6
[System.IO.File]::WriteAllText((Join-Path $outRoot "index.json"), $indexText)

if ($Json) {
    Write-Output $indexText
} else {
    Write-Host ""
    Write-Host ("make-page-fixture: {0} case(s) under {1}" -f $manifest.Count, $outRoot)
    Write-Host ("  serve it with: cargo run -p bit-cli-core --example loopback-fileserver -- --root {0} --port {1}" -f `
            $Root, $BaseUrl.Split(':')[-1])
}
exit 0
