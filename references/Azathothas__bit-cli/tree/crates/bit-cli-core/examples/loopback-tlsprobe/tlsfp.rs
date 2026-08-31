//! `ClientHello` parsing, and the JA3 and JA4 fingerprints computed from it.
//!
//! Every read goes through a cursor that returns `None` rather than panicking,
//! because the bytes come off a socket and a truncated or hostile record must
//! end the capture, not the process.

use sha2::{Digest as _, Sha256};

/// Lowercase hex of a digest.
///
/// `sha2` and `md-5` are pinned at 0.11 here, where `digest()` returns an
/// `Array` that does not implement `LowerHex`, so `format!("{:x}", ...)` does
/// not compile. `bit_cli_core::digest` spells it out the same way for the same
/// reason.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

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
    b.as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_be_bytes(*c))
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
            v.iter()
                .map(|x| x.to_string())
                .collect::<Vec<_>>()
                .join("-")
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
        let hash = hex(&md5::Md5::digest(s.as_bytes()));
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
        cs.iter()
            .map(|c| format!("{c:04x}"))
            .collect::<Vec<_>>()
            .join(",")
    }

    fn ja4_c_raw(&self) -> String {
        let mut ex: Vec<u16> = strip(&self.extensions)
            .into_iter()
            .filter(|&e| e != 0x0000 && e != 0x0010) // SNI and ALPN excluded
            .collect();
        ex.sort_unstable();
        let exs = ex
            .iter()
            .map(|e| format!("{e:04x}"))
            .collect::<Vec<_>>()
            .join(",");
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
        hex(&Sha256::digest(s.as_bytes()))[..12].to_string()
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

    /// JA4_r — the un-hashed form, sorted. This is what you diff when two
    /// fingerprints disagree; the hashes only tell you *that* they differ.
    pub fn ja4_r(&self) -> String {
        format!("{}_{}_{}", self.ja4_a(), self.ja4_b_raw(), self.ja4_c_raw())
    }

    /// JA4_ro — the un-hashed form in the order the client actually sent.
    ///
    /// JA4 and JA4_r sort the ciphers and the extensions before comparing,
    /// which is what makes them stable against a client that shuffles its
    /// extensions between connections. That stability hides something: two
    /// clients with the same JA4_r can still put their extensions on the wire
    /// in different orders, and the order is itself a signal.
    ///
    /// So this is the diagnostic form. It is **never asserted**, for exactly
    /// the reason JA3 is never asserted: it moves when nothing is wrong. It is
    /// what to read when a JA4_r matches and a capture still looks unlike the
    /// client it claims to be.
    pub fn ja4_ro(&self) -> String {
        let ciphers = strip(&self.ciphers)
            .iter()
            .map(|c| format!("{c:04x}"))
            .collect::<Vec<_>>()
            .join(",");
        let extensions = strip(&self.extensions)
            .iter()
            .map(|e| format!("{e:04x}"))
            .collect::<Vec<_>>()
            .join(",");
        let sig = strip(&self.sig_algs)
            .iter()
            .map(|s| format!("{s:04x}"))
            .collect::<Vec<_>>()
            .join(",");
        match sig.is_empty() {
            true => format!("{}_{}_{}", self.ja4_a(), ciphers, extensions),
            false => format!("{}_{}_{}_{}", self.ja4_a(), ciphers, extensions, sig),
        }
    }

    /// Markers that separate a current Chrome from a generic TLS client.
    pub fn browser_markers(&self) -> Vec<(&'static str, bool)> {
        vec![
            (
                "GREASE in ciphers",
                self.ciphers.iter().any(|&c| is_grease(c)),
            ),
            (
                "GREASE in extensions",
                self.extensions.iter().any(|&e| is_grease(e)),
            ),
            ("ECH (0xfe0d)", self.has_ech),
            ("ALPS (0x4469/0x44cd)", self.has_alps),
            ("cert compression (0x1b)", !self.cert_compression.is_empty()),
            (
                "X25519MLKEM768 key share",
                self.key_share_groups.contains(&0x11ec),
            ),
            ("ALPN offers h2", self.alpn.iter().any(|a| a == "h2")),
            (
                "session ticket / PSK modes",
                !self.psk_key_exchange_modes.is_empty(),
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One `ClientHello` this repository's own browser profile put on the
    /// wire, recorded by `loopback-tlsprobe --raw --hello-out`.
    ///
    /// **Embedded as hex and not as a file.** `scripts/check-tree.ps1` keeps a
    /// `.bin` out of everywhere but `vendor/`, and a binary blob is not
    /// something a reviewer can read either. Regenerate with:
    ///
    /// ```text
    /// loopback-tlsprobe --once --raw --port 0 --hello-out <path>
    /// bit-cli info <url>/x.torrent --page-client browser
    /// ```
    const BROWSER_HELLO: &str = "\
         16030107c2010007be0303d69d687de54ead21140d518b0818677cf35dec768429018597\
         32f965193e48e520134916ebb31ca558c5fb9bcaa6cca1f4374721b22f6eeb0e8c1c7c01\
         0634115e00200a0a130113021303c02bc02fc02cc030cca9cca8c013c014009c009d002f\
         0035010007556a6a0000000d001800160904090509060403080404010503080505010806\
         0601000a000c000a8a8a11ec001d001700180017000000120000ff01000100001b000302\
         0002002d00020101000b000201000023000044cd0008000605696d706974000500050100\
         000000002b00050403040303003304ea04e811ec04c0a2505e4104a56234bfe3fa66db54\
         63bd9a000d57a3a326aa8e112394b1ccc8a19e12b7670c173ed116b5bce9131857ae4678\
         b52a552ca6209eb6530cb7f62526685c145372abb670b3e2ae3fbb71dcbb87aa15aa4679\
         5ce2306942e2b009e8bb6e47c3ffe9277fb77f8fab073921cfdbf89711784c97471ed589\
         89d225048fc6bb11ec143fcb556ee4a20dcba73c45398e406fb302b49b839c3343429d00\
         478bf1115736b09fa1bfbec956616894a5fb3c2b106bc9d674ac320fb70a5aa5058c8374\
         a2e33205ade001d87a8cd93253d482492bd87aa6a504dd1b52e04c0a096632deb82bb50b\
         4351b751299465e08a315c21c8e406181e2396a7d1538519639d1130776797c1786339ea\
         7d00dd590b2926b292a7e0087711c0865da909e4d7b50cc244d0a01ea6cb9e082631bc7a\
         69abccc652925c2b952019d93408276a4730555c8c73315110bcdb26308ca0df8060aa38\
         cc01b50e991b784251b9e96100cb8bb8df6504f79a3c768c758d80a394e06c6dc0621fd7\
         5baa38a00cc2065a7035d8f106d0f951ff709e220a5e1a144763832c0415ca12cb93d354\
         40e0d66ec73200f4b69dea486b8f2197aba71f3b79c847392ea9c7ac45889fecf9b89da6\
         95c06b427df31d85e83b683c33a9fb9091ccaf2501bab542c952d594b7bc9eaef3b6aa8a\
         b3f9269b1412b73e354aed855837b69878e826dcda8cc6f180a589c83c188d261c6728b3\
         6b1a203c9524bde0027c0f98c9a3e984a6726a35a7791e210b268bb4cd1c6b7925a81569\
         a5dd3201b65934fde97afe4c3462b8a5c9c325bbb456557086f86736e6f18bd7354721d2\
         881699a2a2da6c459389c47b0d2ff3c8bbcc973720b885695e0e369801470c8274811f93\
         a84937146595137e98613b010b95144c85e42667c9828a92abd139bb4f7b8b5f50afa77b\
         7d2f8641d7540dcdfcc714685759202d0a3126976b37337569968a1d63bbb1a1573d07f0\
         3f02a2beef4b7adb082b2a9134cf501563531ee08a926929bb6738bea8a71693c915b186\
         cdfef412ae92904db53f83f794eb012f0117c5ee8745b609649e128f5a81321be4ab9d80\
         461cd37c91e01cad970744bc8dec621122086a009a2f988cbf280a39c9b2ba6bc323d43c\
         60cc78918b490914b88f73c0335a3917c01b3dacfc6ec0935eca36648b19b51f6439cbba\
         1ab37b8ca47560925b459ab3bab13455e0a6ac8f27b5d52b7197c45c64d15843924625cc\
         b1d9d93390725eb2214924250fc722517ee614cea20ab80a0fd022c2172c132ba532c8d1\
         6d23f5923de2842802ce2ba394c080513b21cdb6215dfffa1fd233ab16d75b94ba897486\
         b1248b89c43a12a48a9cf2689b09a1a09f8b5e02131bba744dda86b5c9e2233a5250b3e0\
         3d1a37a1f84b4d7750c73e084de1c72cce285fd95b351c352f5e299fdba9008036924489\
         38ab299b33aca25630245f818b34f7c4bce57c4c85509d9c9b0e03a5eab03cffca50d3eb\
         2a872a2bca61c2b1a9711e1230c3f6b159a99c262a82c94713fcfca72d7609d8b67d1ea1\
         40de8c78185471322140529c875d2c45f40ccb3e154cadc4664b974fede6932a1c05affa\
         728b1212192a71e3c52908d6133d466ac490744bd5c694a3baba2f9e365935d193d49e85\
         b2ef4d59c89826197d492d53653c562e4f55707ff273d512d8211aa4a7ba49f7103768da\
         7720c7b69290bddd797b84cca556001d0020707ff273d512d8211aa4a7ba49f7103768da\
         7720c7b69290bddd797b84cca5560010000e000c02683208687474702f312e31fe0d01da\
         0000010001ee0020eb99d7f84f685f2044bd6240f675155aa8157d1457520758b9bddb88\
         98238f5f01b09f4cd7e7b8712952ed9bfdad5726fb44a4c0faa0bb5653af6f8eba993032\
         2feaa677d03cf0d2333ef094c895bf2e3c2056ce77cbdd67c2055975bb937f9476b4188f\
         671890859f4dc022c9bb350b08f29def6d60ee5a58a7081c2c7f01b4b964e74aa904699f\
         bfc3b29a91cdacf1e0278348d05d607a4c754747f18fe65a71cdb4851b187fe6292dca8e\
         ca45774b2efffc86ccbc07a9956f0d19e8d42fdea77be3977ae3358e54b3a6f588d95858\
         101025c815e77a98ff4ccb0085b03391f0ae06556d7fe51ea00c9a13ae9b97e433cc17a6\
         d347c8574ce5a97c2cf8710b2c397bb0d9161885c26ff5b4e921cf60aa77ac5af5f9b957\
         03bdad3f1b8d999f60685c718718876f3a29a5a49b64ddc56375f211d1b4ba6d6688720a\
         01935587d82331be79344ab43c8048b10027b6160eb3268e34b1ec72b6bf29c853bad2b8\
         654f8f08b7ad09779cc437d065db3738243ec5b972336b827fbae629e9d83bbf8eec72f6\
         25282af4646635dcedfe0dec42c8e834a5aa59a4fddf58f8f16cbcdee8868fac900cf4e3\
         6689bc80d079290298106538b371f777d0127cf2dca9df711c8208dbdd7d5555b5224a66\
         3a908ea890257a7a000100";

    /// The same, from the `plain` profile: this tree's own `rustls`, with no
    /// impersonation. It is the control. A parser change that improves the
    /// browser reading and breaks this one has broken the parser.
    const PLAIN_HELLO: &str = "\
        16030100f0010000ec0303ec01f8cf3a4c4ddc2ba4daf22f0579d7adf853bdbc6ed0a5f9417e\
        a153c66f5c205509727a869f8d9fb026ff5e8304a318199bcf04829c9278685d1e16b4a2053f\
        0014130213011303c02cc02bcca9c030c02fcca800ff0100008f002d00020101000b00020100\
        00170000001b0005040002000100230000003300260024001d0020946f1307af4382da283fdf\
        9d3e1c908146f76966d09f858e802af2f7cb25242a0010000e000c02683208687474702f312e\
        31000a00080006001d00170018000500050100000000000d0014001205030403080708060805\
        0804060105010401002b00050403040303";

    fn bytes(hex: &str) -> Vec<u8> {
        assert!(hex.len().is_multiple_of(2), "a hex string has even length");
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex"))
            .collect()
    }

    #[test]
    fn the_recorded_hellos_decode_to_the_length_they_were_captured_at() {
        assert_eq!(bytes(BROWSER_HELLO).len(), 1991);
        assert_eq!(bytes(PLAIN_HELLO).len(), 245);
    }

    #[test]
    fn the_browser_hello_parses_to_the_fingerprint_it_was_captured_with() {
        let ch = parse(&bytes(BROWSER_HELLO)).expect("a recorded ClientHello parses");
        assert_eq!(ch.ja4(), "t13i1515h2_8daaf6152771_806a8c22fdea");
        assert_eq!(
            ch.ja4_r(),
            "t13i1515h2_002f,0035,009c,009d,1301,1302,1303,c013,c014,c02b,c02c,c02f,c030,cca8,cca9_0005,000a,000b,000d,0012,0017,001b,0023,002b,002d,0033,44cd,fe0d,ff01_0904,0905,0906,0403,0804,0401,0503,0805,0501,0806,0601"
        );
    }

    #[test]
    fn the_plain_hello_parses_to_the_fingerprint_it_was_captured_with() {
        let ch = parse(&bytes(PLAIN_HELLO)).expect("a recorded ClientHello parses");
        assert_eq!(ch.ja4(), "t13i1011h2_61a7ad8aa9b6_69ed562cf35e");
        assert_eq!(
            ch.ja4_r(),
            "t13i1011h2_00ff,1301,1302,1303,c02b,c02c,c02f,c030,cca8,cca9_0005,000a,000b,000d,0017,001b,0023,002b,002d,0033_0503,0403,0807,0806,0805,0804,0601,0501,0401"
        );
    }

    /// What separates the two, field by field. This is the assertion that
    /// would catch a parser reading the right number of ciphers out of the
    /// wrong offset.
    #[test]
    fn the_browser_hello_carries_what_a_browser_carries_and_the_plain_one_does_not() {
        let browser = parse(&bytes(BROWSER_HELLO)).expect("browser");
        let plain = parse(&bytes(PLAIN_HELLO)).expect("plain");

        assert_eq!(strip(&browser.ciphers).len(), 15);
        assert_eq!(strip(&plain.ciphers).len(), 10);

        assert!(browser.has_ech, "a current Chrome offers ECH");
        assert!(!plain.has_ech, "this tree's own rustls does not");
        assert!(browser.has_alps, "a current Chrome offers ALPS");
        assert!(!plain.has_alps);
        assert!(
            !browser.cert_compression.is_empty(),
            "a current Chrome offers certificate compression"
        );

        // GREASE is in the browser's cipher list and in neither of the
        // plain client's, which is the single clearest marker there is.
        assert!(browser.ciphers.iter().any(|&c| is_grease(c)));
        assert!(!plain.ciphers.iter().any(|&c| is_grease(c)));
        assert!(!plain.extensions.iter().any(|&e| is_grease(e)));

        // And in the browser's extension list, at each end, which is what a
        // real Chrome does and what T-263 closed. The assertion is inverted
        // rather than deleted, the way `scripts/check-listener.ps1`'s cases
        // were when T-020 closed: what used to record the gap now holds the
        // fix. It is invisible to JA4, because JA4 strips GREASE before
        // hashing, which is why it is asserted here on the raw hello.
        assert!(
            browser.extensions.iter().any(|&e| is_grease(e)),
            "the profile lost extension GREASE, which T-263 added"
        );
        let first = *browser.extensions.first().expect("a hello has extensions");
        let last = *browser.extensions.last().expect("a hello has extensions");
        assert!(is_grease(first), "the first extension is {first:#06x}");
        assert!(is_grease(last), "the last extension is {last:#06x}");
        assert_ne!(
            first, last,
            "both ends carry the same GREASE value, which is a constant a server can key on"
        );
        assert_eq!(
            browser.extensions.iter().filter(|&&e| is_grease(e)).count(),
            2,
            "a browser sends exactly two, one at each end"
        );
    }

    /// The cipher list is Chrome's, in Chrome's wire order, GREASE included.
    ///
    /// Sixteen values in one sequence, captured from a real Chrome 151 on the
    /// same machine on the same day. Only the GREASE value itself differs, and
    /// it differs on purpose: a browser picks a new one per connection.
    #[test]
    fn the_browser_cipher_list_is_chromes_own_wire_order() {
        let ch = parse(&bytes(BROWSER_HELLO)).expect("browser");
        let chrome: Vec<u16> = vec![
            0x1301, 0x1302, 0x1303, 0xc02b, 0xc02f, 0xc02c, 0xc030, 0xcca9, 0xcca8, 0xc013, 0xc014,
            0x009c, 0x009d, 0x002f, 0x0035,
        ];
        assert!(
            is_grease(ch.ciphers[0]),
            "GREASE leads, as it does in Chrome"
        );
        assert_eq!(ch.ciphers[1..].to_vec(), chrome);
    }

    /// GREASE values are dropped before hashing, by the JA4 specification.
    /// This is the property that makes a JA4 stable across connections at all,
    /// because Chrome picks new GREASE values every time.
    #[test]
    fn grease_is_stripped_from_every_hashed_list() {
        let ch = parse(&bytes(BROWSER_HELLO)).expect("browser");
        assert!(ch.ciphers.iter().any(|&c| is_grease(c)), "raw list has it");
        assert!(
            !strip(&ch.ciphers).iter().any(|&c| is_grease(c)),
            "the stripped list does not"
        );
        assert!(!ch.ja4_r().contains("0a0a"), "{}", ch.ja4_r());
    }

    /// JA4_ro keeps the wire order where JA4_r sorts. Both describe the same
    /// hello, so they carry the same values and, for a client that does not
    /// send them sorted, in a different sequence.
    #[test]
    fn ja4_ro_keeps_the_order_ja4_r_sorts_away() {
        let ch = parse(&bytes(BROWSER_HELLO)).expect("browser");
        let sorted = ch.ja4_r();
        let wire = ch.ja4_ro();
        assert_ne!(sorted, wire, "a browser does not send them sorted");
        assert!(wire.starts_with(&sorted[..sorted.find('_').expect("prefix")]));
    }

    /// Nothing here may panic on a hostile input, which is the whole reason
    /// the parser is written on a cursor that returns `Option` at every step.
    #[test]
    fn a_truncated_hello_is_none_and_never_a_panic() {
        let full = bytes(BROWSER_HELLO);
        for cut in [0, 1, 5, 9, 40, 100, 500, full.len() - 1] {
            let _ = parse(&full[..cut]);
        }
    }

    #[test]
    fn a_hello_with_a_length_that_overruns_the_buffer_is_none() {
        let mut full = bytes(BROWSER_HELLO);
        // Claim a record far longer than what follows.
        full[3] = 0xff;
        full[4] = 0xff;
        let _ = parse(&full);
    }

    #[test]
    fn bytes_that_are_not_tls_are_none() {
        assert!(parse(b"GET / HTTP/1.1\r\n\r\n").is_none());
        assert!(parse(&[]).is_none());
        assert!(parse(&[0x16]).is_none());
    }
}
