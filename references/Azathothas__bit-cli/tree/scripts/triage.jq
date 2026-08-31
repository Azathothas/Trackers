def cat:
  (.title + " " + ((.body // "")[0:600])) | ascii_downcase;
.[] | . as $i | cat as $t |
{
  n: $i.number, s: $i.state, t: $i.title, url: $i.url,
  closed: $i.closedAt,
  c: (
    [
      (if ($t|test("web ?seed|url-list|httpseed|bep ?19|bep ?17|http source|mirror")) then "webseed" else empty end),
      (if ($t|test("peer|handshake|choke|unchoke|swarm|connection")) then "peers" else empty end),
      (if ($t|test("slow|speed|throughput|performance|fast|bottleneck|piece pick|endgame|pipelin")) then "performance" else empty end),
      (if ($t|test("disk|write|fsync|mmap|sparse|preallocat|file exists|os error|storage|cache")) then "disk-io" else empty end),
      (if ($t|test("memory|leak|rss|fd |file descriptor|exhaust|oom")) then "memory" else empty end),
      (if ($t|test("dht")) then "dht" else empty end),
      (if ($t|test("tracker|announce|scrape|udp tracker")) then "trackers" else empty end),
      (if ($t|test("upnp|nat|port forward|reachab|firewall")) then "network" else empty end),
      (if ($t|test("bep ?[0-9]|utp|ipv6|encryption|protocol|magnet|metadata|v2 |bittorrent v2")) then "bep" else empty end),
      (if ($t|test("windows|win32|ntfs|path|filename|reserved")) then "windows" else empty end),
      (if ($t|test("seed|upload|ratio|superseed")) then "seeding" else empty end),
      (if ($t|test("create|metainfo|torrent file|torrent creat")) then "create" else empty end),
      (if ($t|test("bench|measure|metric|telemetry|stat")) then "bench" else empty end)
    ] | unique
  )
}
