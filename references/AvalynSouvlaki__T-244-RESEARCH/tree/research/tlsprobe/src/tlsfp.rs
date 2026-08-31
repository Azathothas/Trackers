//! ClientHello parsing and the JA3 / JA4 fingerprints.


use sha2::{Digest as _, Sha256};

/// A GREASE value (RFC 8701): both bytes equal, low nibble `a`.
pub fn is_grease(v: u16) -> bool {
    v & 0x0f0f == 0x0a0a && (v >> 8) == (v & 0xff)
}

fn strip(v: &[u16]) -> Vec<u16> {
    v.iter().copied().filter(|&x| !is_grease(x)).collect()
}

#[derive(Default, Debug)]
pub struct ClientHello {
    pub legacy_version: u16,
    pub ciphers: Vec<u16>,
    pub extensions: Vec<u16>,
    pub curves: Vec<u16>,
    pub point_formats: Vec<u8>,
    pub alpn: Vec<String>,
    pub sig_algs: Vec<u16>,
    pub supported_versions: Vec<u16>,
    pub sni: Option<String>,
    pub cert_compression: Vec<u16>,
    pub key_share_groups: Vec<u16>,
    pub psk_key_exchange_modes: Vec<u8>,
    pub record_version: u16,
    pub has_ech: bool,
    pub has_alps: bool,
}

/// Slice-with-cursor that never panics on a truncated or hostile record.
struct Cur<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Cur<'a> {
    fn new(b: &'a [u8]) -> Self {
        Cur { b, p: 0 }
    }
    fn u8(&mut self) -> Option<u8> {
        let v = *self.b.get(self.p)?;
        self.p += 1;
        Some(v)
    }
    fn u16(&mut self) -> Option<u16> {
        let v = u16::from_be_bytes(self.b.get(self.p..self.p + 2)?.try_into().ok()?);
        self.p += 2;
        Some(v)
    }
    fn u24(&mut self) -> Option<u32> {
        let s = self.b.get(self.p..self.p + 3)?;
        self.p += 3;
        Some(((s[0] as u32) << 16) | ((s[1] as u32) << 8) | s[2] as u32)
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.p..self.p.checked_add(n)?)?;
        self.p += n;
        Some(s)
    }
    fn skip(&mut self, n: usize) -> Option<()> {
        self.take(n).map(|_| ())
    }
}

fn u16_list(b: &[u8]) -> Vec<u16> {
    b.chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect()
}

/// Parse a TLS record containing a ClientHello.
pub fn parse(buf: &[u8]) -> Option<ClientHello> {
    let mut c = Cur::new(buf);
    let mut ch = ClientHello::default();

    if c.u8()? != 0x16 {
        return None; // not a handshake record
    }
    ch.record_version = c.u16()?;
    let rec_len = c.u16()? as usize;
    // Guard against a ClientHello fragmented across records: we only ever peek
    // one buffer, so a short read is a parse failure, not a silent truncation.
    if buf.len() < 5 + rec_len {
        return None;
    }
    if c.u8()? != 0x01 {
        return None; // not a ClientHello
    }
    c.u24()?; // handshake length
    ch.legacy_version = c.u16()?;
    c.skip(32)?; // random
    let sid = c.u8()? as usize;
    c.skip(sid)?;

    let cs_len = c.u16()? as usize;
    ch.ciphers = u16_list(c.take(cs_len)?);
    let comp = c.u8()? as usize;
    c.skip(comp)?;

    // Extensions are optional in the wire format (SSLv3-era hellos omit them).
    let ext_total = match c.u16() {
        Some(n) => n as usize,
        None => return Some(ch),
    };
    let end = c.p + ext_total;

    while c.p < end {
        let et = c.u16()?;
        let el = c.u16()? as usize;
        let body = c.take(el)?;
        ch.extensions.push(et);

        match et {
            0x0000 => {
                // server_name: list(2) type(1) len(2) host
                let mut s = Cur::new(body);
                s.u16()?;
                if s.u8() == Some(0) {
                    let n = s.u16()? as usize;
                    ch.sni = s.take(n).map(|h| String::from_utf8_lossy(h).into_owned());
                }
            }
            0x000a => {
                let mut s = Cur::new(body);
                let n = s.u16()? as usize;
                ch.curves = u16_list(s.take(n)?);
            }
            0x000b => {
                let mut s = Cur::new(body);
                let n = s.u8()? as usize;
                ch.point_formats = s.take(n)?.to_vec();
            }
            0x000d => {
                let mut s = Cur::new(body);
                let n = s.u16()? as usize;
                ch.sig_algs = u16_list(s.take(n)?);
            }
            0x0010 => {
                let mut s = Cur::new(body);
                let total = s.u16()? as usize;
                let inner = s.take(total)?;
                let mut q = Cur::new(inner);
                while q.p < inner.len() {
                    let n = q.u8()? as usize;
                    ch.alpn
                        .push(String::from_utf8_lossy(q.take(n)?).into_owned());
                }
            }
            0x002b => {
                let mut s = Cur::new(body);
                let n = s.u8()? as usize;
                ch.supported_versions = u16_list(s.take(n)?);
            }
            0x002d => {
                let mut s = Cur::new(body);
                let n = s.u8()? as usize;
                ch.psk_key_exchange_modes = s.take(n)?.to_vec();
            }
            0x0033 => {
                let mut s = Cur::new(body);
                let total = s.u16()? as usize;
                let inner = s.take(total)?;
                let mut q = Cur::new(inner);
                while q.p < inner.len() {
                    let g = q.u16()?;
                    let n = q.u16()? as usize;
                    q.skip(n)?;
                    ch.key_share_groups.push(g);
                }
            }
            0x001b => {
                let mut s = Cur::new(body);
                let n = s.u8()? as usize;
                ch.cert_compression = u16_list(s.take(n)?);
            }
            0xfe0d => ch.has_ech = true,
            0x4469 | 0x44cd => ch.has_alps = true,
            _ => {}
        }
    }
    Some(ch)
}

impl ClientHello {
    /// Highest offered TLS version, preferring `supported_versions`.
    pub fn effective_version(&self) -> u16 {
        strip(&self.supported_versions)
            .into_iter()
            .max()
            .unwrap_or(self.legacy_version)
    }

    fn ja4_version(&self) -> &'static str {
        match self.effective_version() {
            0x0304 => "13",
            0x0303 => "12",
            0x0302 => "11",
            0x0301 => "10",
            0x0300 => "s3",
            _ => "00",
        }
    }

    /// JA3 (Salesforce). `filter_grease` is **not** in the original spec, but
    /// every modern implementation applies it — without it Chrome's JA3 changes
    /// on every connection, since Chrome randomises its GREASE values.
    pub fn ja3(&self, filter_grease: bool) -> (String, String) {
        let f = |v: &[u16]| {
            let v = if filter_grease { strip(v) } else { v.to_vec() };
            v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join("-")
        };
        let s = format!(
            "{},{},{},{},{}",
            self.legacy_version,
            f(&self.ciphers),
            f(&self.extensions),
            f(&self.curves),
            self.point_formats
                .iter()
                .map(|x| x.to_string())
                .collect::<Vec<_>>()
                .join("-")
        );
        let hash = format!("{:x}", md5::Md5::digest(s.as_bytes()));
        (s, hash)
    }

    /// JA4 `a` segment: `t<ver><d|i><ciphers><exts><alpn>`.
    fn ja4_a(&self) -> String {
        let nc = strip(&self.ciphers).len().min(99);
        // The extension *count* includes SNI and ALPN; only the `c` hash drops them.
        let ne = strip(&self.extensions).len().min(99);
        let alpn = match self.alpn.first() {
            // Per the JA4 spec the marker is the first and last byte of the
            // first ALPN value, so "http/1.1" -> "h1", "h2" -> "h2".
            Some(a) if !a.is_empty() => {
                let b = a.as_bytes();
                format!("{}{}", b[0] as char, b[b.len() - 1] as char)
            }
            _ => "00".to_string(),
        };
        format!(
            "t{}{}{:02}{:02}{}",
            self.ja4_version(),
            if self.sni.is_some() { 'd' } else { 'i' },
            nc,
            ne,
            alpn
        )
    }

    fn ja4_b_raw(&self) -> String {
        let mut cs = strip(&self.ciphers);
        cs.sort_unstable();
        cs.iter().map(|c| format!("{c:04x}")).collect::<Vec<_>>().join(",")
    }

    fn ja4_c_raw(&self) -> String {
        let mut ex: Vec<u16> = strip(&self.extensions)
            .into_iter()
            .filter(|&e| e != 0x0000 && e != 0x0010) // SNI and ALPN excluded
            .collect();
        ex.sort_unstable();
        let exs = ex.iter().map(|e| format!("{e:04x}")).collect::<Vec<_>>().join(",");
        // Signature algorithms keep their ORIGINAL order — they are not sorted.
        let sig = strip(&self.sig_algs)
            .iter()
            .map(|s| format!("{s:04x}"))
            .collect::<Vec<_>>()
            .join(",");
        if sig.is_empty() {
            exs
        } else {
            format!("{exs}_{sig}")
        }
    }

    fn trunc12(s: &str) -> String {
        if s.is_empty() {
            return "000000000000".into();
        }
        format!("{:x}", Sha256::digest(s.as_bytes()))[..12].to_string()
    }

    /// Hashed JA4.
    pub fn ja4(&self) -> String {
        format!(
            "{}_{}_{}",
            self.ja4_a(),
            Self::trunc12(&self.ja4_b_raw()),
            Self::trunc12(&self.ja4_c_raw())
        )
    }

    /// JA4_r — the un-hashed form. This is what you diff when two fingerprints
    /// disagree; the hashes only tell you *that* they differ.
    pub fn ja4_r(&self) -> String {
        format!("{}_{}_{}", self.ja4_a(), self.ja4_b_raw(), self.ja4_c_raw())
    }

    /// Markers that separate a current Chrome from a generic TLS client.
    pub fn browser_markers(&self) -> Vec<(&'static str, bool)> {
        vec![
            ("GREASE in ciphers", self.ciphers.iter().any(|&c| is_grease(c))),
            ("GREASE in extensions", self.extensions.iter().any(|&e| is_grease(e))),
            ("ECH (0xfe0d)", self.has_ech),
            ("ALPS (0x4469/0x44cd)", self.has_alps),
            ("cert compression (0x1b)", !self.cert_compression.is_empty()),
            ("X25519MLKEM768 key share", self.key_share_groups.contains(&0x11ec)),
            ("ALPN offers h2", self.alpn.iter().any(|a| a == "h2")),
            ("session ticket / PSK modes", !self.psk_key_exchange_modes.is_empty()),
        ]
    }
}
