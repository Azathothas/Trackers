# Serving a torrent's payload from S3, and what it costs

An S3 bucket is already a web seed. It answers ranged `GET` with `206`, it
serves any key over HTTPS, and it needs no code in front of it. What it does
not do is tell you whether the keys match what the torrent will ask for, or
what a request costs in latency, or how many requests a download will make.

Every command here was run against a real bucket while this was written, and
the numbers below are that run. The bucket is `noaa-goes16`, an AWS Open Data
bucket that is public and free to read, so the same commands reproduce. One
object stands in for a payload:

```bash
curl -sO https://noaa-goes16.s3.amazonaws.com/ABI-L1b-RadC/2024/001/00/OR_ABI-L1b-RadC-M6C01_G16_s20240010001173_e20240010003546_c20240010004005.nc
```

That is 2,410,916 bytes. A torrent over it, with a piece length small enough
that a sweep has something to sweep:

```bash
bit-cli create OR_ABI-L1b-RadC-M6C01_G16_s20240010001173_e20240010003546_c20240010004005.nc --output goes16.torrent --piece-length 256KiB --no-creation-date
```

Everything after this points a web seed at the bucket the object came from, and
every command below uses that torrent and the object's URL. The shell variable
holding the URL is the one this page calls `$OBJECT`:

```bash
OBJECT=https://noaa-goes16.s3.amazonaws.com/ABI-L1b-RadC/2024/001/00/OR_ABI-L1b-RadC-M6C01_G16_s20240010001173_e20240010003546_c20240010004005.nc
```

## The key layout is what breaks first

BEP 19 composes a per-file URL by appending the torrent's `name` and the file's
`path` to the base, and only when the base ends in `/`. S3 keys are flat
strings with slashes in them, so the composed URL either names a key that
exists or it does not, and nothing about the bucket makes it more likely.

Print the URLs before a byte moves:

```bash
bit-cli webseed list album.torrent --web-seed "https://bucket.s3.amazonaws.com/pub/"
```

Three layouts, three flags:

| the bucket holds | what to pass |
| --- | --- |
| keys that mirror the torrent's directory tree under a prefix | `--web-seed "https://bucket.s3.amazonaws.com/pub/"` |
| one object holding the whole single-file payload | `--web-seed-exact "https://bucket.s3.amazonaws.com/pub/blob.bin"` |
| keys that are neither, such as a content hash per file | `--web-seed-template` |

`--web-seed-exact` is what a single object wants and it is the case above: the
base URL is used unchanged rather than composed, so the object key needs no
relationship to the torrent's name at all.

## One request says whether the bucket works

```bash
bit-cli webseed test goes16.torrent --web-seed-exact "$OBJECT"
```

```text
source               https://noaa-goes16.s3.amazonaws.com/ABI-L1b-RadC/...
  requested          https://noaa-goes16.s3.amazonaws.com/ABI-L1b-RadC/...
  scope              *
  status             206
  ranges             yes
  length             2.30 MiB (matches the torrent)
  server             AmazonS3
  http               HTTP/1.1
  tls                TLSv1_3 TLS13_AES_128_GCM_SHA256
  handshake          connect 269ms, tls 267ms
  alpn               http/1.1
  ttfb               876ms
  total              1414ms
```

That is one request for one byte of payload. Every line is worth reading and
three of them are not obvious.

**`alpn http/1.1`.** `bit-cli` offers `h2` first and S3 chose HTTP/1.1. So a
web seed on S3 gets no request multiplexing over one connection, and
concurrency has to come from more connections. That is measured rather than
assumed: the offer is at
[`../../crates/bit-cli-core/src/webseed/probe.rs`](../../crates/bit-cli-core/src/webseed/probe.rs),
where `alpn_protocols` is `h2` then `http/1.1`.

**`handshake connect 269ms, tls 267ms`.** `connect` is DNS resolution plus the
TCP connection. `tls` is the handshake on top of it. They come from a
connection the probe opens for the purpose, so they are representative of what
the path costs rather than a subtotal of the `ttfb` beside them. Do not
subtract one from the other.

**`length 2.30 MiB (matches the torrent)`.** The entity length S3 reports
against what the torrent says the payload is. A mismatch here is the failure
that would otherwise appear as every piece failing its hash, and it is caught
before any byte is trusted.

`--head` sends `HEAD` instead of a one-byte ranged `GET`. It is cheaper and it
proves less: a bucket policy can allow `HEAD` and deny `GET`, and a proxy can
answer `HEAD` from metadata it does not have the object for.

## What the numbers say about tuning

Two sweeps against the same object, eight seconds per step.

At the default chunk size, 4 MiB, which is larger than this object, so each
request pulls all of it:

```bash
bit-cli webseed probe goes16.torrent --web-seed-exact "$OBJECT" --concurrency-sweep 1,2,4,8 --duration 8s
```

```text
  chunk              4.00 MiB
  best               4.32 MiB/s at concurrency 4
  CONC  REQS  ERRS  RATE          P50     P90     P99      P99.9    MAX      TTFB P50
  1     1     0     910.44 KiB/s  2587ms  2587ms  2587ms   2587ms   2587ms   921ms
  2     2     0     1.35 MiB/s    2737ms  3413ms  3413ms   3413ms   3413ms   302ms
  4     10    0     4.32 MiB/s    531ms   2677ms  3351ms   3351ms   3351ms   280ms
  8     17    0     2.71 MiB/s    855ms   4983ms  14423ms  14423ms  14423ms  302ms
```

At one piece per request:

```bash
bit-cli webseed probe goes16.torrent --web-seed-exact "$OBJECT" --web-seed-chunk-size 256KiB --concurrency-sweep 1,4,16 --duration 8s
```

```text
  chunk              256.00 KiB
  best               5.62 MiB/s at concurrency 16
  CONC  REQS  ERRS  RATE          P50    P90     P99     P99.9   MAX     TTFB P50
  1     3     0     282.77 KiB/s  582ms  1825ms  1825ms  1825ms  1825ms  304ms
  4     17    0     698.87 KiB/s  313ms  1693ms  5935ms  5935ms  5935ms  305ms
  16    83    0     5.62 MiB/s    313ms  1103ms  1826ms  1826ms  1826ms  295ms
```

**Read the `TTFB P50` column first.** It is about 300ms in both sweeps, at
every concurrency, and it does not move when the chunk size changes by a factor
of sixteen. That is the cost of asking S3 for anything from this machine, and
it is the number the rest follows from.

**So the chunk size sets the floor and concurrency is what lifts it.** One
connection asking for 256 KiB at a time and waiting 300ms for each answer
cannot exceed about 850 KiB/s no matter how fast the link is. The measurement
says 283 KiB/s, because the 300ms is time to the first byte and the rest of the
chunk still has to arrive. Sixteen of those in flight reach 5.62 MiB/s.

**The two sweeps disagree about the best concurrency and both are right.** At 4
MiB per request, four in flight is the peak and eight is worse: 2.71 MiB/s with
a p99 of 14.4 seconds. At 256 KiB per request, sixteen is still climbing. The
pairing is what matters, not either number alone, and `--web-seed-chunk-size`
and `--web-seed-concurrency` are the two flags that set it.

**A p99 that runs away is the signal to stop raising concurrency.** Throughput
falling is the second symptom and the slower one to see. In the first sweep the
p99 went from 3,351ms to 14,423ms between the peak and the step after it.

Pass the winning pair to a real download:

```bash
bit-cli download goes16.torrent --web-seed-exact "$OBJECT" --web-seed-only --web-seed-chunk-size 256KiB --web-seed-concurrency 4
```

## What a download costs in requests

S3 charges per request. `--json` carries the count:

```text
elapsed_ms       7450
downloaded       2.30 MiB
http_requests    10
http_bytes       2410916
blocks           148
whole_pieces     10
connections      1
retries          0
```

Ten pieces, ten requests, and `http_bytes` equal to the payload to the byte:
nothing was fetched twice and nothing was fetched and discarded. That is the
property to check on a real payload, because the failure it catches is a
retried source re-downloading ranges it already had.

**The default chunk size is the request count.** At 4 MiB the same download is
one request; at 256 KiB it is ten. For a 20 GiB payload that is 5,000 requests
against 80,000, and the latency argument above pulls the other way. The tension
is real and the two sweeps are how to settle it for a given bucket.

`webseed fetch` shows the request a single piece actually produces, with a
`curl` line that reproduces it:

```bash
bit-cli webseed fetch goes16.torrent --web-seed-exact "$OBJECT" --piece 0
```

```text
verified             true
request              bytes=0-2410915
  status             206
  total              2359ms
  curl               curl -sS -D - -H 'Range: bytes=0-2410915' -o /dev/null https://noaa-goes16.s3.amazonaws.com/...
```

The range covers the whole object for a 256 KiB piece, because the default
chunk size is larger than the object. On a large payload that is the request
being amortised over several pieces, and on a small one it is bytes paid for
and thrown away.

## A redirect is a source with somebody else's rules

An S3-compatible endpoint that redirects is common, and the chain is worth
printing before it is trusted. `dl.min.io` is a MinIO server that redirects to
GitHub releases:

```bash
bit-cli webseed test goes16.torrent --web-seed-exact "https://dl.min.io/server/minio/release/linux-amd64/minio"
```

```text
  status             206
  ranges             yes
  length             105.85 MiB (the torrent says 2.30 MiB)
  resolved to        https://release-assets.githubusercontent.com/...&se=2026-08-24T11%3A27%3A06Z&sig=...
  redirect           302 -> https://github.com/minio/minio/releases/download/...
  redirect           302 -> https://release-assets.githubusercontent.com/...
  server             Windows-Azure-Blob/1.0 Microsoft-HTTPAPI/2.0
  http               HTTP/1.1
  tls                TLSv1_3 TLS13_AES_128_GCM_SHA256
  handshake          connect 19ms, tls 16ms
  alpn               h2
  ttfb               426ms
  total              1884ms
```

Four things in one output, and the exit code was 6, no usable sources.

**The TLS line describes the last host, not the first.** `alpn h2` and a 19ms
connect are Azure Blob, which is where the second redirect landed. A latency
budget written from the first hostname would be wrong by an order of magnitude.

**The final URL carries an expiry.** `se=2026-08-24T11:27:06Z` in the query is
a presigned signature valid for one hour. A web seed pointing at a URL like
that works until it does not, and the failure arrives as `403` mid-download.
A presigned URL is a source with a deadline, and `--web-seed-cooldown` does not
help with it because retrying does not renew the signature.

**Three providers answered one request.** MinIO, then GitHub, then Azure Blob.
Each hop is latency on every request for the whole download, and any of them
may handle `Range` differently from the one before it.

**The length check is what caught it.** 105.85 MiB against the torrent's 2.30
MiB, so the source was refused rather than served. Without that check this is
the failure that reads as every piece failing its hash.

## ETag, and why it is not always the MD5

The object above has `ETag: "00ef71519c0472c824d9e77185515694"`, and that is
the MD5 of the bytes:

```bash
curl -sS -I "$OBJECT" | grep -i etag
```

That holds for an object uploaded in one part. An object uploaded with the
multipart API has an ETag of the form `<hex>-<count>`, which is a hash of the
part hashes and is not the MD5 of anything a client can compute without knowing
the part size. So an ETag is usable as a change detector and is not usable as a
checksum.

It matters here for one reason: `If-Range`. A client resuming a partial read
sends the ETag it had, and an origin whose ETag changed is entitled to answer
`200` with the whole entity instead of `206` with the range. Re-uploading an
object changes its ETag even when the bytes are identical, so a deploy that
re-uploads unchanged files breaks resumption for everyone mid-download.

None of that reaches the payload. Every piece is hash-checked against the
torrent before it counts, whatever the source claimed. See
[`../integrity.md`](../integrity.md).

## The S3-compatible backends, and where they differ

`bit-cli` sends ordinary ranged `GET` over HTTPS and nothing S3-specific, so
anything that answers those works. The differences that reach a web seed:

| backend | what changes for a web seed |
| --- | --- |
| AWS S3 | charges per request and per byte out. `alpn http/1.1` above, so concurrency means connections |
| Cloudflare R2 | no egress charge, and a custom domain serves ranges without a Worker. See [`cloudflare-webseed.md`](cloudflare-webseed.md) |
| MinIO | self-hosted, so the request cost is your own. Check whether it is behind a redirect, as above |
| Backblaze B2 | the S3-compatible endpoint answers ranges; the native B2 endpoint is a different URL shape |
| Ceph RGW, Garage | ranged `GET` is the whole requirement, and both answer it |

Three settings are worth checking on any of them:

- **No transcoding.** `bit-cli` sends `Accept-Encoding: identity` on every web
  seed request, because a proxy that compresses changes what a byte range
  means. A backend or CDN configured to compress the payload path returns
  wrong bytes from a healthy origin.
- **Public read, or credentials passed.** `--web-seed-auth` takes
  `basic:user:pass`, `bearer:TOKEN` or `netrc`, and `--web-seed-header` sets
  anything else. SigV4 request signing is not implemented, so a private bucket
  needs a presigned URL or a proxy in front, with the expiry caveat above.
- **Rate limits, and which statuses to retry.** `429` and `503` are the ones
  worth retrying and `--web-seed-retry-status` is where to say so.
  `--web-seed-max-errors` is how many failures retire a source and
  `--web-seed-cooldown` is how long it waits before it is tried again.

## What was measured here and what was not

The bucket is public and the object is a NOAA satellite data file, used because
it is a stable public object of a convenient size. One ranged `GET` is what
each check costs and no payload was downloaded except the 2.3 MiB object itself
and one verification run over it.

Whether a CDN in front of the bucket served the request from cache **is**
answerable, and `webseed test` is where it is answered: the report carries
`x-cache`, `age`, `etag`, `cache-control`, `x-amz-request-id` and `x-amz-id-2`
from the exchange it already made. `x-amz-request-id` is the value an S3
support ticket asks for first, and it cannot be recovered after the request, so
capturing it on the request that failed is the only chance there is.
[`../webseed.md`](../webseed.md) has the whole reported set and the flag that
adds to it.

Not measured: a bucket with request-payer enabled, a bucket in a region far
from the client, and the same object through CloudFront. The commands are the
same; the numbers are the reader's own.

Related pages: [`cloudflare-webseed.md`](cloudflare-webseed.md) for R2 and
Workers, [`mirror-benchmark.md`](mirror-benchmark.md) for attributing the cost
across four stages, and [`../webseed.md`](../webseed.md) for the whole binding
grammar.
