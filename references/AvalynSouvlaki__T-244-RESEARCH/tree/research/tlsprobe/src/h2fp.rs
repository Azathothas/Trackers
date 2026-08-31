//! HTTP/2 frame parsing, the Akamai fingerprint, and header order.
//!
//! The Akamai fingerprint is four `|`-joined parts:
//!   `SETTINGS | WINDOW_UPDATE | PRIORITY | PSEUDO_HEADER_ORDER`
//! e.g. `1:65536;2:0;4:6291456;6:262144|15663105|0|m,a,s,p`
//!
//! This is the half of a client's identity that a TLS-only capture misses, and
//! it is exactly where a rustls-based impersonator is most likely to diverge
//! from a BoringSSL-based one: the TLS layer and the H2 layer are different
//! crates, and only the TLS half is usually tuned.

use crate::huffman;

pub const PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

#[rustfmt::skip]
const STATIC_TABLE: [&str; 61] = [
    ":authority", ":method", ":method", ":path", ":path", ":scheme", ":scheme",
    ":status", ":status", ":status", ":status", ":status", ":status", ":status",
    "accept-charset", "accept-encoding", "accept-language", "accept-ranges", "accept",
    "access-control-allow-origin", "age", "allow", "authorization", "cache-control",
    "content-disposition", "content-encoding", "content-language", "content-length",
    "content-location", "content-range", "content-type", "cookie", "date", "etag",
    "expect", "expires", "from", "host", "if-match", "if-modified-since",
    "if-none-match", "if-range", "if-unmodified-since", "last-modified", "link",
    "location", "max-forwards", "proxy-authenticate", "proxy-authorization", "range",
    "referer", "refresh", "retry-after", "server", "set-cookie",
    "strict-transport-security", "transfer-encoding", "user-agent", "vary", "via",
    "www-authenticate",
];

fn setting_name(id: u16) -> &'static str {
    match id {
        1 => "HEADER_TABLE_SIZE",
        2 => "ENABLE_PUSH",
        3 => "MAX_CONCURRENT_STREAMS",
        4 => "INITIAL_WINDOW_SIZE",
        5 => "MAX_FRAME_SIZE",
        6 => "MAX_HEADER_LIST_SIZE",
        8 => "ENABLE_CONNECT_PROTOCOL",
        9 => "NO_RFC7540_PRIORITIES",
        _ => "UNKNOWN",
    }
}

#[derive(Default)]
pub struct H2Fingerprint {
    pub settings: Vec<(u16, u32)>,
    pub window_update: Option<u32>,
    pub priorities: Vec<(u32, u8, u32, u8)>, // stream, exclusive, dep, weight
    pub pseudo_order: Vec<String>,
    pub headers: Vec<String>,
    pub saw_headers: bool,
}

/// HPACK variable-length integer (RFC 7541 §5.1).
fn hpack_int(b: &[u8], p: &mut usize, prefix: u8) -> Option<u64> {
    let mask = (1u16 << prefix) - 1;
    let mut v = (*b.get(*p)? & mask as u8) as u64;
    *p += 1;
    if v < mask as u64 {
        return Some(v);
    }
    let mut shift = 0;
    loop {
        let byte = *b.get(*p)?;
        *p += 1;
        v = v.checked_add(((byte & 0x7f) as u64) << shift)?;
        shift += 7;
        if byte & 0x80 == 0 {
            return Some(v);
        }
        if shift > 56 {
            return None;
        }
    }
}

/// Read an HPACK string literal, Huffman-decoding when flagged.
fn hpack_str(b: &[u8], p: &mut usize) -> Option<String> {
    let huff = *b.get(*p)? & 0x80 != 0;
    let len = hpack_int(b, p, 7)? as usize;
    let raw = b.get(*p..p.checked_add(len)?)?;
    *p += len;
    if huff {
        huffman::decode(raw)
    } else {
        String::from_utf8(raw.to_vec()).ok()
    }
}

/// Decode just the header *names*, in order. Values are skipped — this is a
/// fingerprint tool, and header values are the one part that carries the
/// caller's actual data.
fn decode_header_names(block: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    let mut p = 0usize;
    while p < block.len() {
        let first = block[p];
        let name = if first & 0x80 != 0 {
            // Indexed header field.
            match hpack_int(block, &mut p, 7) {
                Some(i) if i >= 1 && (i as usize) <= STATIC_TABLE.len() => {
                    STATIC_TABLE[i as usize - 1].to_string()
                }
                Some(0) => return names, // index 0 is a protocol error
                Some(i) => format!("<dynamic:{i}>"),
                None => return names,
            }
        } else {
            // Literal, in one of three framings that differ only in prefix width.
            let prefix = if first & 0xc0 == 0x40 {
                6 // with incremental indexing
            } else if first & 0xe0 == 0x20 {
                // Dynamic table size update — no header here, just resize.
                if hpack_int(block, &mut p, 5).is_none() {
                    return names;
                }
                continue;
            } else {
                4 // without indexing / never indexed
            };
            let idx = match hpack_int(block, &mut p, prefix) {
                Some(i) => i,
                None => return names,
            };
            let n = if idx == 0 {
                match hpack_str(block, &mut p) {
                    Some(s) => s,
                    None => return names,
                }
            } else if (idx as usize) <= STATIC_TABLE.len() {
                STATIC_TABLE[idx as usize - 1].to_string()
            } else {
                format!("<dynamic:{idx}>")
            };
            // Skip the value to reach the next field.
            if hpack_str(block, &mut p).is_none() {
                return names;
            }
            n
        };
        names.push(name);
    }
    names
}

/// Parse everything the client sent after the connection preface.
pub fn parse(buf: &[u8]) -> H2Fingerprint {
    let mut fp = H2Fingerprint::default();
    let mut p = if buf.starts_with(PREFACE) { PREFACE.len() } else { 0 };

    while p + 9 <= buf.len() {
        let len = ((buf[p] as usize) << 16) | ((buf[p + 1] as usize) << 8) | buf[p + 2] as usize;
        let ftype = buf[p + 3];
        let flags = buf[p + 4];
        let stream = u32::from_be_bytes([buf[p + 5] & 0x7f, buf[p + 6], buf[p + 7], buf[p + 8]]);
        p += 9;
        let Some(body) = buf.get(p..p + len) else { break };
        p += len;

        match ftype {
            0x4 => {
                // SETTINGS — ACKs carry no payload and must not be recorded.
                if flags & 0x1 == 0 {
                    for c in body.chunks_exact(6) {
                        fp.settings.push((
                            u16::from_be_bytes([c[0], c[1]]),
                            u32::from_be_bytes([c[2], c[3], c[4], c[5]]),
                        ));
                    }
                }
            }
            0x8 => {
                // WINDOW_UPDATE — the Akamai fingerprint uses the connection-level one.
                if body.len() >= 4 && stream == 0 {
                    fp.window_update =
                        Some(u32::from_be_bytes([body[0] & 0x7f, body[1], body[2], body[3]]));
                }
            }
            0x2 => {
                if body.len() >= 5 {
                    let dep = u32::from_be_bytes([body[0] & 0x7f, body[1], body[2], body[3]]);
                    fp.priorities.push((stream, body[0] >> 7, dep, body[4]));
                }
            }
            0x1 => {
                fp.saw_headers = true;
                let mut b = body;
                if flags & 0x20 != 0 && b.len() >= 5 {
                    // PRIORITY flag prepends an exclusive/dependency/weight block.
                    let dep = u32::from_be_bytes([b[0] & 0x7f, b[1], b[2], b[3]]);
                    fp.priorities.push((stream, b[0] >> 7, dep, b[4]));
                    b = &b[5..];
                }
                if flags & 0x8 != 0 && !b.is_empty() {
                    // PADDED: one length byte up front, that many bytes at the end.
                    let pad = b[0] as usize;
                    b = b.get(1..b.len().saturating_sub(pad)).unwrap_or(&[]);
                }
                for n in decode_header_names(b) {
                    if n.starts_with(':') {
                        fp.pseudo_order.push(n.clone());
                    }
                    fp.headers.push(n);
                }
            }
            _ => {}
        }
    }
    fp
}

impl H2Fingerprint {
    /// The Akamai HTTP/2 fingerprint string.
    pub fn akamai(&self) -> String {
        let s = self
            .settings
            .iter()
            .map(|(k, v)| format!("{k}:{v}"))
            .collect::<Vec<_>>()
            .join(";");
        let w = self.window_update.map(|v| v.to_string()).unwrap_or_else(|| "00".into());
        let pr = if self.priorities.is_empty() {
            "0".to_string()
        } else {
            self.priorities
                .iter()
                .map(|(s, e, d, wt)| format!("{s}:{e}:{d}:{wt}"))
                .collect::<Vec<_>>()
                .join(",")
        };
        // Pseudo-headers reduce to their first letter: :method -> m, :path -> p.
        let ph = self
            .pseudo_order
            .iter()
            .filter_map(|h| h.chars().nth(1))
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .join(",");
        format!("{s}|{w}|{pr}|{ph}")
    }

    pub fn settings_pretty(&self) -> Vec<String> {
        self.settings
            .iter()
            .map(|(k, v)| format!("{k} {} = {v}", setting_name(*k)))
            .collect()
    }
}
