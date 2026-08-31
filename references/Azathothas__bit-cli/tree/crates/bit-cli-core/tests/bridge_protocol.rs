//! The web seed bridge against a peer that is not `librqbit`.
//!
//! Every other test of the bridge puts a real `librqbit` session on the far
//! end. That session numbers its BEP 10 extensions exactly the way the
//! bridge's own decoder does, because both come from the same crate, and that
//! is precisely the arrangement that hides a defect of the shape vortex
//! PR 103 found: an extension map keyed by **our** id and looked up with
//! **theirs**. Getting that backwards is silent against any peer whose
//! numbering happens to match, and it meant extensions had never once worked
//! against qBittorrent.
//!
//! The session here is written by hand, byte by byte, and numbers its
//! extensions deliberately unlike anything the bridge or `librqbit` uses. It
//! also never calls the serializer the bridge calls, so nothing in this file
//! can agree with the bridge by construction.
//!
//! See `TODO/peers.md`, T-166.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use bit_cli_core::layout::Layout;
use bit_cli_core::webseed::binding::{BindingSet, Origin, SourceSpec};
use bit_cli_core::webseed::bridge::{self, BridgeParams, BridgeStatus};
use bit_cli_core::webseed::fetch::Fetcher;
use librqbit_core::Id20;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Four pieces of 32 KiB, so a piece index and a block offset are distinct
/// numbers and an off-by-one in either is visible.
const PIECE_LENGTH: u32 = 32 * 1024;
const PIECE_COUNT: u32 = 4;
const BLOCK: u32 = 16 * 1024;

/// Peer protocol message ids, written out rather than imported. Importing them
/// would make this test agree with the bridge by sharing a constant.
const MSG_CHOKE: u8 = 0;
const MSG_UNCHOKE: u8 = 1;
const MSG_INTERESTED: u8 = 2;
const MSG_BITFIELD: u8 = 5;
const MSG_REQUEST: u8 = 6;
const MSG_PIECE: u8 = 7;
const MSG_EXTENDED: u8 = 20;
/// BEP 6, written out for the same reason every other id here is: importing
/// them would make this test agree with the bridge by sharing a constant.
const MSG_HAVE_ALL: u8 = 14;
const MSG_REJECT_REQUEST: u8 = 16;

/// `librqbit` 9.0.0's own receive-side numbering, from
/// `librqbit-peer-protocol/src/lib.rs`: `MY_EXTENDED_UT_PEX = 1` and
/// `MY_EXTENDED_UT_METADATA = 3`. The session below advertises **different**
/// numbers for the same names, which is the whole point, and then also sends
/// messages carrying these, which is what a peer that got the direction
/// backwards would do.
const LIBRQBIT_UT_PEX: u8 = 1;
const LIBRQBIT_UT_METADATA: u8 = 3;

/// What this session advertises in its own `m`. Every value differs from the
/// constants above and from every other value here, so no two lookups can
/// succeed by coincidence.
const OUR_UT_METADATA: u8 = 2;
const OUR_UPLOAD_ONLY: u8 = 4;
const OUR_LT_DONTHAVE: u8 = 7;

/// Deterministic bytes, so a served block can be checked against the source.
fn content(len: usize, seed: u8) -> Vec<u8> {
    let mut state = u64::from(seed).wrapping_add(0x9E37_79B9_7F4A_7C15);
    (0..len)
        .map(|_| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 33) as u8
        })
        .collect()
}

/// One length-prefixed peer message.
fn frame(id: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = ((payload.len() as u32) + 1).to_be_bytes().to_vec();
    out.push(id);
    out.extend_from_slice(payload);
    out
}

/// One BEP 10 extension message: message id 20, then the extension id the
/// **receiver** advertised for that extension, then a bencoded body.
fn extended(extension_id: u8, body: &[u8]) -> Vec<u8> {
    let mut payload = vec![extension_id];
    payload.extend_from_slice(body);
    frame(MSG_EXTENDED, &payload)
}

/// A parsed peer message: the id and its payload.
struct Frame {
    id: u8,
    payload: Vec<u8>,
}

/// Read exactly one message, skipping keep-alives.
///
/// Bounded by a deadline rather than by a message count, so a bridge that
/// sends nothing fails the test with a timeout instead of hanging the suite.
async fn read_frame(stream: &mut TcpStream) -> Frame {
    loop {
        let mut len = [0u8; 4];
        tokio::time::timeout(Duration::from_secs(20), stream.read_exact(&mut len))
            .await
            .expect("the bridge sent no frame within twenty seconds")
            .expect("read a length prefix");
        let len = u32::from_be_bytes(len) as usize;
        if len == 0 {
            continue;
        }
        assert!(len < 1024 * 1024, "absurd frame length {len}");
        let mut buf = vec![0u8; len];
        tokio::time::timeout(Duration::from_secs(20), stream.read_exact(&mut buf))
            .await
            .expect("the bridge sent a truncated frame")
            .expect("read a frame body");
        return Frame {
            id: buf[0],
            payload: buf[1..].to_vec(),
        };
    }
}

/// Everything a hand-written session needs to talk to one bridge.
struct Session {
    stream: TcpStream,
    /// The payload the source serves, so a `piece` can be checked.
    data: Vec<u8>,
    /// Kept so the source file outlives the bridge.
    _source: tempfile::TempDir,
}

impl Session {
    /// Bind a listener, point a bridge at it, and complete the handshake.
    ///
    /// The source is a local file rather than an HTTP server: this test is
    /// about the peer protocol, and a `file://` source removes a whole stub
    /// server from the things that could fail it.
    /// The default session, which does not speak BEP 6.
    async fn start() -> Self {
        Self::start_with(false).await
    }

    /// `fast` sets BEP 6's reserved bit, which is the last reserved byte and
    /// `0x04`. BEP 10's is byte 5 and `0x10`: two different bytes, and a test
    /// that set the wrong one would negotiate nothing and still pass a
    /// bitfield assertion.
    async fn start_with(fast: bool) -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();

        let source = tempfile::tempdir().unwrap();
        let data = content((PIECE_LENGTH * PIECE_COUNT) as usize, 17);
        let path = source.path().join("payload.bin");
        std::fs::write(&path, &data).unwrap();

        let layout = Arc::new(Layout::from_lengths(
            "payload.bin",
            false,
            PIECE_LENGTH,
            [("payload.bin".to_string(), data.len() as u64)],
        ));
        let info_hash = Id20::new([0x5Au8; 20]);
        let hash_hex = info_hash.as_string();

        let mut spec = SourceSpec::new(
            bit_cli_core::webseed::local::url_of(&path),
            Origin::CommandLine,
        );
        spec.mode = bit_cli_core::webseed::Mode::Exact;
        let set = BindingSet::resolve(&layout, &hash_hex, &[spec]).unwrap();
        let binding = &set.bindings[0];

        let params = BridgeParams::for_binding(
            addr,
            info_hash,
            // A session peer id the bridge must not collide with. The bridge
            // generates its own under a fixed prefix, so any value that is not
            // one of those works.
            Id20::new([0x11u8; 20]),
            &layout,
            binding,
            4,
        );
        let fetcher =
            Arc::new(Fetcher::new(binding.clone(), layout.clone(), hash_hex, 4, false).unwrap());
        tokio::spawn(bridge::run(
            params,
            fetcher,
            Arc::new(BridgeStatus::default()),
        ));

        let (mut stream, _) = tokio::time::timeout(Duration::from_secs(20), listener.accept())
            .await
            .expect("the bridge did not dial within twenty seconds")
            .unwrap();

        // The bridge dials, so its handshake arrives first.
        let mut theirs = [0u8; 68];
        tokio::time::timeout(Duration::from_secs(20), stream.read_exact(&mut theirs))
            .await
            .expect("no handshake within twenty seconds")
            .expect("read the handshake");
        assert_eq!(theirs[0], 19, "pstrlen");
        assert_eq!(&theirs[1..20], b"BitTorrent protocol");
        assert_eq!(
            theirs[25] & 0x10,
            0x10,
            "the bridge has to set the BEP 10 bit, or the extended handshake \
             that carries the BEP 21 flag is not allowed to follow"
        );
        assert_eq!(
            theirs[27] & 0x04,
            0x04,
            "the bridge has to set the BEP 6 bit, or no session can negotiate \
             the fast extension with it"
        );
        assert_eq!(&theirs[28..48], &info_hash.0, "info hash");

        // Ours back, with the extension bit set, so a session that speaks BEP
        // 10 is what the bridge is answering.
        let mut ours = Vec::with_capacity(68);
        ours.push(19);
        ours.extend_from_slice(b"BitTorrent protocol");
        let mut reserved = [0u8; 8];
        reserved[5] = 0x10;
        if fast {
            reserved[7] = 0x04;
        }
        ours.extend_from_slice(&reserved);
        ours.extend_from_slice(&info_hash.0);
        ours.extend_from_slice(&[0x11u8; 20]);
        stream.write_all(&ours).await.unwrap();

        Self {
            stream,
            data,
            _source: source,
        }
    }

    /// Our extended handshake, numbering every extension unlike the bridge.
    ///
    /// BEP 10's `m` says which id **the sender of a message to us** must use.
    /// So these are ids the bridge would have to write, and they are not ids
    /// the bridge may read. A reader that confuses the two directions is
    /// exactly vortex PR 103.
    async fn send_our_extended_handshake(&mut self) {
        let body = format!(
            "d1:md11:lt_donthavei{OUR_LT_DONTHAVE}e11:upload_onlyi{OUR_UPLOAD_ONLY}e11:ut_metadatai{OUR_UT_METADATA}ee4:reqqi500e1:v8:fake/1.0e"
        );
        self.stream
            .write_all(&extended(0, body.as_bytes()))
            .await
            .unwrap();
    }
}

/// The greeting order, which is what vortex PR 156 is about: a message that
/// arrived in the same TCP read as the handshake was processed before the
/// bitfield had been queued, so `Interested` could precede `Bitfield`.
///
/// The bridge's order is the extended handshake, then the bitfield, then
/// unchoke, and the extended handshake being first is deliberate rather than
/// an exception to the rule. BEP 10 puts the extended handshake in the
/// handshaking sequence, and it is what carries the BEP 21 `upload_only` flag
/// that tells the session it is looking at a partial seed rather than a
/// leecher. The rule this asserts is the one that matters: **no ordinary peer
/// message precedes the bitfield.**
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_bitfield_precedes_every_peer_message_after_the_handshake() {
    let mut session = Session::start().await;

    let first = read_frame(&mut session.stream).await;
    assert_eq!(
        first.id, MSG_EXTENDED,
        "the extended handshake comes first, because it carries BEP 21"
    );
    assert_eq!(
        first.payload[0], 0,
        "extension id 0 is the extended handshake itself"
    );

    let second = read_frame(&mut session.stream).await;
    assert_eq!(
        second.id, MSG_BITFIELD,
        "the bitfield is the first ordinary peer message"
    );
    assert_eq!(
        second.payload.len(),
        (PIECE_COUNT as usize).div_ceil(8),
        "one bit per piece and no more"
    );
    assert_eq!(
        second.payload[0] & 0xF0,
        0xF0,
        "a source covering the whole payload announces every piece"
    );

    let third = read_frame(&mut session.stream).await;
    assert_eq!(
        third.id, MSG_UNCHOKE,
        "unchoke follows the bitfield, so the session may start requesting"
    );

    // The bridge only ever seeds, so neither of these may appear at all.
    assert_ne!(third.id, MSG_INTERESTED);
    assert_ne!(third.id, MSG_CHOKE);
}

/// The BEP 10 directions, which is vortex PR 103.
///
/// The session advertises `ut_metadata = 2`, `upload_only = 4` and
/// `lt_donthave = 7`, then sends extension messages under those ids **and**
/// under `librqbit`'s own receive-side numbering, 1 and 3. A bridge that ever
/// indexes one numbering with the other's key has five chances to do it here.
///
/// The assertion is behavioural rather than structural, because the thing that
/// must survive is the connection: after all of that traffic the bridge still
/// answers a `request` with the right bytes at the right offset. A misrouted
/// extension id desynchronises the framer or trips the catch-all into a
/// disconnect, and either one fails the read below.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_peer_that_numbers_its_extensions_differently_is_still_served() {
    let mut session = Session::start().await;

    // Drain the greeting.
    for _ in 0..3 {
        read_frame(&mut session.stream).await;
    }

    session.send_our_extended_handshake().await;

    // Every id that could collide if a lookup went the wrong way, sent as a
    // peer that got the direction backwards would send them.
    for id in [
        OUR_UT_METADATA,
        OUR_UPLOAD_ONLY,
        OUR_LT_DONTHAVE,
        LIBRQBIT_UT_PEX,
        LIBRQBIT_UT_METADATA,
    ] {
        session
            .stream
            .write_all(&extended(id, b"de"))
            .await
            .unwrap();
    }

    // An ordinary message too, so the catch-all is exercised by both kinds.
    session
        .stream
        .write_all(&frame(MSG_INTERESTED, &[]))
        .await
        .unwrap();

    // The proof: a real request, answered correctly, after all of that.
    let index: u32 = 2;
    let begin: u32 = BLOCK;
    let mut request = Vec::new();
    request.extend_from_slice(&index.to_be_bytes());
    request.extend_from_slice(&begin.to_be_bytes());
    request.extend_from_slice(&BLOCK.to_be_bytes());
    session
        .stream
        .write_all(&frame(MSG_REQUEST, &request))
        .await
        .unwrap();

    let piece = loop {
        let got = read_frame(&mut session.stream).await;
        if got.id == MSG_PIECE {
            break got;
        }
        assert_ne!(
            got.id, MSG_CHOKE,
            "the bridge choked instead of serving, which is what a misrouted \
             extension id looks like from here"
        );
    };

    assert_eq!(
        u32::from_be_bytes(piece.payload[0..4].try_into().unwrap()),
        index
    );
    assert_eq!(
        u32::from_be_bytes(piece.payload[4..8].try_into().unwrap()),
        begin
    );
    let offset = (index * PIECE_LENGTH + begin) as usize;
    assert_eq!(
        &piece.payload[8..],
        &session.data[offset..offset + BLOCK as usize],
        "the bytes have to be the source's, at the offset the request named"
    );
}

/// Every extension id, not only the ones a peer is likely to choose.
///
/// The bridge advertises an empty `m`, so **no** incoming extension id is one
/// of its own and every one of them has to be ignored. Before T-166 that was
/// not true, and the two that ended the connection are the two that name the
/// defect: id 3, which `librqbit`'s decoder reads as `ut_metadata`, and id 0,
/// which it reads as an extended handshake. Neither type was ever advertised
/// by this bridge, and a body that did not parse as one ended a web seed
/// connection over a message the bridge had already said it does not speak.
///
/// All 256 ids on one connection rather than a sample on 256 connections: the
/// claim is about the whole space, and a sample is how 1 and 3 stayed hidden.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_extension_id_can_end_the_connection() {
    let mut session = Session::start().await;
    for _ in 0..3 {
        read_frame(&mut session.stream).await;
    }
    session.send_our_extended_handshake().await;

    for id in 0..=u8::MAX {
        // An empty dictionary, which is well-formed bencode and is not a
        // well-formed body for any type `librqbit` might decode it as.
        session
            .stream
            .write_all(&extended(id, b"de"))
            .await
            .unwrap();
    }

    // The realistic shape of the backwards direction: a peer that looked up
    // `ut_metadata` in its own numbering instead of ours and sent a genuine
    // request under it. Well-formed for the type, and still not a message this
    // bridge ever offered to receive.
    session
        .stream
        .write_all(&extended(
            LIBRQBIT_UT_METADATA,
            b"d8:msg_typei0e5:piecei0ee",
        ))
        .await
        .unwrap();
    session
        .stream
        .write_all(&extended(LIBRQBIT_UT_PEX, b"d5:added0:e"))
        .await
        .unwrap();

    let index: u32 = 1;
    let mut request = Vec::new();
    request.extend_from_slice(&index.to_be_bytes());
    request.extend_from_slice(&0u32.to_be_bytes());
    request.extend_from_slice(&BLOCK.to_be_bytes());
    session
        .stream
        .write_all(&frame(MSG_REQUEST, &request))
        .await
        .unwrap();

    let piece = loop {
        let got = read_frame(&mut session.stream).await;
        if got.id == MSG_PIECE {
            break got;
        }
    };
    let offset = (index * PIECE_LENGTH) as usize;
    assert_eq!(
        &piece.payload[8..],
        &session.data[offset..offset + BLOCK as usize],
        "the bridge still serves the right bytes after every extension id"
    );
}

/// BEP 6. A source whose scope is the whole torrent announces it in two bytes
/// rather than one bit per piece, and only against a session that negotiated
/// the extension.
///
/// Both directions are asserted on the same fixture, because the interesting
/// failure is announcing `have all` to a peer that does not know the message:
/// that is a dropped connection rather than a smaller greeting, and a test
/// that only checked the negotiated case would not see it.
///
/// See `TODO/bep-coverage.md`, T-100.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_complete_source_announces_have_all_only_when_bep_6_is_negotiated() {
    // Negotiated: two bytes.
    let mut session = Session::start_with(true).await;
    let first = read_frame(&mut session.stream).await;
    assert_eq!(
        first.id, MSG_EXTENDED,
        "the extended handshake is still first"
    );
    let announce = read_frame(&mut session.stream).await;
    assert_eq!(
        announce.id, MSG_HAVE_ALL,
        "a complete source has to say have all rather than send a bitfield"
    );
    assert!(
        announce.payload.is_empty(),
        "have all carries no payload: {:?}",
        announce.payload
    );

    // Not negotiated: the bitfield, exactly as before.
    let mut plain = Session::start().await;
    let first = read_frame(&mut plain.stream).await;
    assert_eq!(first.id, MSG_EXTENDED);
    let announce = read_frame(&mut plain.stream).await;
    assert_eq!(
        announce.id, MSG_BITFIELD,
        "a session that did not negotiate BEP 6 has to get a bitfield"
    );
    assert_eq!(
        announce.payload.len(),
        (PIECE_COUNT as usize).div_ceil(8),
        "one bit per piece, rounded up to a byte"
    );
}

/// BEP 6. A request this source cannot answer is refused with `reject request`
/// and the connection stays up.
///
/// This is the half of T-100 that the extension exists for. Before it, the
/// only way to refuse was to stop talking, and a partial seed being asked for
/// a piece it does not hold is a normal thing rather than a protocol error.
///
/// The out-of-scope piece is one past the end of the torrent, so the refusal
/// is deterministic: nothing has to lose a file first and no scheduling
/// decision is involved.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_out_of_scope_request_is_rejected_rather_than_ending_the_connection() {
    let mut session = Session::start_with(true).await;
    for _ in 0..3 {
        read_frame(&mut session.stream).await;
    }

    let outside: u32 = PIECE_COUNT + 5;
    let mut request = Vec::new();
    request.extend_from_slice(&outside.to_be_bytes());
    request.extend_from_slice(&0u32.to_be_bytes());
    request.extend_from_slice(&BLOCK.to_be_bytes());
    session
        .stream
        .write_all(&frame(MSG_REQUEST, &request))
        .await
        .unwrap();

    let reject = read_frame(&mut session.stream).await;
    assert_eq!(
        reject.id, MSG_REJECT_REQUEST,
        "the bridge answered id {} instead of rejecting",
        reject.id
    );
    assert_eq!(
        reject.payload, request,
        "a rejection names the request it refuses, byte for byte"
    );

    // The connection is still there, which is the whole point: a real request
    // after the refusal is answered with the source's own bytes.
    let index: u32 = 3;
    let mut good = Vec::new();
    good.extend_from_slice(&index.to_be_bytes());
    good.extend_from_slice(&0u32.to_be_bytes());
    good.extend_from_slice(&BLOCK.to_be_bytes());
    session
        .stream
        .write_all(&frame(MSG_REQUEST, &good))
        .await
        .unwrap();

    let piece = loop {
        let got = read_frame(&mut session.stream).await;
        if got.id == MSG_PIECE {
            break got;
        }
        assert_ne!(got.id, MSG_CHOKE, "the bridge choked after a rejection");
    };
    let offset = (index * PIECE_LENGTH) as usize;
    assert_eq!(
        &piece.payload[8..],
        &session.data[offset..offset + BLOCK as usize],
        "the bridge still serves after refusing"
    );
}
