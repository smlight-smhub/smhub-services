//! ESPHome's native API, as a client (docs/peripherals.md, "Reading ESPHome"): the encrypted
//! frame (Noise NNpsk0 with the hub's API key), the dozen protobuf messages the hub uses, and a
//! connection task that reconnects by itself. `smhub-broker` serves the API on port 6053 for the
//! firmware on the second core; Home Assistant is another client of the same port.

use std::time::Duration;

use serde::Serialize;
use smhub_model::{Error, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::mpsc;

pub const ADDR: &str = "127.0.0.1:6053";
const NOISE: &str = "Noise_NNpsk0_25519_ChaChaPoly_SHA256";
const PROLOGUE: &[u8] = b"NoiseAPIInit\x00\x00";
/// Quiet after which the hub pings the server itself (ESPHome pings only after 60 s).
const QUIET: Duration = Duration::from_secs(30);
/// Silence after which the connection counts as dead.
const SILENCE: Duration = Duration::from_secs(90);
/// Commands waiting to be sent; more are refused.
pub const COMMANDS: usize = 32;
/// Events waiting for the hub.
pub const EVENTS: usize = 64;
/// Entities kept from one listing.
const MAX_ENTITIES: usize = 256;

/// Message types of the API (`api.proto`).
pub mod ty {
    pub const HELLO_REQ: u16 = 1;
    pub const HELLO_RESP: u16 = 2;
    pub const CONNECT_REQ: u16 = 3;
    pub const CONNECT_RESP: u16 = 4;
    pub const DISCONNECT_REQ: u16 = 5;
    pub const DISCONNECT_RESP: u16 = 6;
    pub const PING_REQ: u16 = 7;
    pub const PING_RESP: u16 = 8;
    pub const DEVICE_INFO_REQ: u16 = 9;
    pub const DEVICE_INFO_RESP: u16 = 10;
    pub const LIST_ENTITIES_REQ: u16 = 11;
    pub const LIST_BINARY_SENSOR: u16 = 12;
    pub const LIST_LIGHT: u16 = 15;
    pub const LIST_SWITCH: u16 = 17;
    pub const LIST_DONE: u16 = 19;
    pub const SUBSCRIBE_STATES: u16 = 20;
    pub const BINARY_SENSOR_STATE: u16 = 21;
    pub const LIGHT_STATE: u16 = 24;
    pub const SWITCH_STATE: u16 = 26;
    pub const LIGHT_COMMAND: u16 = 32;
    pub const SWITCH_COMMAND: u16 = 33;
    pub const GET_TIME_REQ: u16 = 36;
    pub const GET_TIME_RESP: u16 = 37;
    pub const LIST_SERVICES: u16 = 41;
    pub const EXECUTE_SERVICE: u16 = 42;
}

// ---------------------------------------------------------------------------------------------
// protobuf, the few wire types the messages use

/// A protobuf message being written. Fields equal to their default are left out, as proto3 does.
#[derive(Default, Debug, Clone)]
pub struct Pb(pub Vec<u8>);

impl Pb {
    fn varint(&mut self, mut v: u64) {
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                self.0.push(b);
                return;
            }
            self.0.push(b | 0x80);
        }
    }

    fn tag(&mut self, field: u32, wire: u8) {
        self.varint((u64::from(field) << 3) | u64::from(wire));
    }

    pub fn uint(&mut self, field: u32, v: u64) -> &mut Self {
        if v != 0 {
            self.tag(field, 0);
            self.varint(v);
        }
        self
    }

    pub fn boolean(&mut self, field: u32, v: bool) -> &mut Self {
        self.uint(field, u64::from(v))
    }

    /// A fixed32 written even when zero (entity keys).
    pub fn fixed32(&mut self, field: u32, v: u32) -> &mut Self {
        self.tag(field, 5);
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }

    pub fn float(&mut self, field: u32, v: f32) -> &mut Self {
        if v != 0.0 {
            self.tag(field, 5);
            self.0.extend_from_slice(&v.to_le_bytes());
        }
        self
    }

    pub fn string(&mut self, field: u32, s: &str) -> &mut Self {
        if !s.is_empty() {
            self.bytes(field, s.as_bytes());
        }
        self
    }

    /// A length-delimited field written even when empty (a repeated sub-message).
    pub fn bytes(&mut self, field: u32, b: &[u8]) -> &mut Self {
        self.tag(field, 2);
        self.varint(b.len() as u64);
        self.0.extend_from_slice(b);
        self
    }
}

/// One field read from a message.
#[derive(Debug, Clone, Copy)]
pub enum Field<'a> {
    Varint(u64),
    Fixed32(u32),
    Fixed64(u64),
    Bytes(&'a [u8]),
}

impl Field<'_> {
    pub fn u64(&self) -> u64 {
        match *self {
            Field::Varint(v) | Field::Fixed64(v) => v,
            Field::Fixed32(v) => u64::from(v),
            Field::Bytes(_) => 0,
        }
    }

    pub fn u32(&self) -> u32 {
        u32::try_from(self.u64() & 0xffff_ffff).unwrap_or(0)
    }

    pub fn boolean(&self) -> bool {
        self.u64() != 0
    }

    pub fn float(&self) -> f32 {
        match *self {
            Field::Fixed32(v) => f32::from_bits(v),
            _ => 0.0,
        }
    }

    pub fn text(&self) -> String {
        match *self {
            Field::Bytes(b) => String::from_utf8_lossy(b).into_owned(),
            _ => String::new(),
        }
    }
}

fn read_varint(b: &mut &[u8]) -> Option<u64> {
    let mut v: u64 = 0;
    for shift in (0..64).step_by(7) {
        let (&byte, rest) = b.split_first()?;
        *b = rest;
        v |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

/// The fields of a message, in order. Stops at the first malformed field.
pub struct Fields<'a>(&'a [u8]);

pub fn fields(b: &[u8]) -> Fields<'_> {
    Fields(b)
}

impl<'a> Iterator for Fields<'a> {
    type Item = (u32, Field<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        if self.0.is_empty() {
            return None;
        }
        let mut b = self.0;
        let key = read_varint(&mut b)?;
        let field = u32::try_from(key >> 3).ok()?;
        let value = match key & 7 {
            0 => Field::Varint(read_varint(&mut b)?),
            1 => {
                let (v, rest) = b.split_first_chunk::<8>()?;
                b = rest;
                Field::Fixed64(u64::from_le_bytes(*v))
            }
            2 => {
                let len = usize::try_from(read_varint(&mut b)?).ok()?;
                if len > b.len() {
                    self.0 = &[];
                    return None;
                }
                let (v, rest) = b.split_at(len);
                b = rest;
                Field::Bytes(v)
            }
            5 => {
                let (v, rest) = b.split_first_chunk::<4>()?;
                b = rest;
                Field::Fixed32(u32::from_le_bytes(*v))
            }
            _ => {
                self.0 = &[];
                return None;
            }
        };
        self.0 = b;
        Some((field, value))
    }
}

// ---------------------------------------------------------------------------------------------
// what the hub sees and sends

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Light,
    Switch,
    BinarySensor,
    Service,
    Other,
}

/// An entity of the firmware, from its listing.
#[derive(Debug, Clone, Serialize)]
pub struct Entity {
    pub kind: Kind,
    pub key: u32,
    pub object_id: String,
    pub name: String,
    /// Light effects the firmware offers.
    pub effects: Vec<String>,
    /// Service arguments: name and type (3 = string).
    pub args: Vec<(String, u32)>,
}

/// A state the firmware reports.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum State {
    Light { key: u32, on: bool, brightness: f32, rgb: [f32; 3], effect: String },
    Switch { key: u32, on: bool },
    BinarySensor { key: u32, on: bool, missing: bool },
}

impl State {
    pub fn key(&self) -> u32 {
        match self {
            State::Light { key, .. } | State::Switch { key, .. } | State::BinarySensor { key, .. } => *key,
        }
    }
}

/// A light command; `None` leaves that part as it is.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LightCommand {
    pub key: u32,
    pub on: Option<bool>,
    pub brightness: Option<f32>,
    pub rgb: Option<[f32; 3]>,
    pub effect: Option<String>,
    pub transition_ms: Option<u32>,
    pub flash_ms: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Light(LightCommand),
    Switch { key: u32, on: bool },
    /// A user-defined service with string arguments only (the firmware's RTTTL service).
    Service { key: u32, strings: Vec<String> },
}

/// What the connection reports to the hub.
#[derive(Debug, Clone)]
pub enum Event {
    Connected { name: String, server: String },
    Entities(Vec<Entity>),
    State(State),
    Disconnected(String),
}

/// API 1.14: older versions are logged as outdated by ESPHome 2026, and 1.14 servers leave out
/// the object id (the hub finds its devices by name as well).
fn hello() -> Pb {
    let mut p = Pb::default();
    p.string(1, "smhubd").uint(2, 1).uint(3, 14);
    p
}

pub fn encode(cmd: &Command) -> (u16, Pb) {
    let mut p = Pb::default();
    match cmd {
        Command::Light(c) => {
            p.fixed32(1, c.key);
            if let Some(on) = c.on {
                p.boolean(2, true).boolean(3, on);
            }
            if let Some(b) = c.brightness {
                p.boolean(4, true).float(5, b.clamp(0.0, 1.0));
            }
            if let Some([r, g, b]) = c.rgb {
                p.boolean(6, true).float(7, r.clamp(0.0, 1.0)).float(8, g.clamp(0.0, 1.0)).float(9, b.clamp(0.0, 1.0));
            }
            if let Some(t) = c.transition_ms {
                p.boolean(14, true).uint(15, u64::from(t));
            }
            if let Some(f) = c.flash_ms {
                p.boolean(16, true).uint(17, u64::from(f));
            }
            if let Some(e) = &c.effect {
                p.boolean(18, true).string(19, e);
            }
            (ty::LIGHT_COMMAND, p)
        }
        Command::Switch { key, on } => {
            p.fixed32(1, *key).boolean(2, *on);
            (ty::SWITCH_COMMAND, p)
        }
        Command::Service { key, strings } => {
            p.fixed32(1, *key);
            for s in strings {
                let mut a = Pb::default();
                a.string(4, s);
                p.bytes(2, &a.0);
            }
            (ty::EXECUTE_SERVICE, p)
        }
    }
}

/// An entity from a listing message; `None` for other message types.
pub fn decode_entity(t: u16, data: &[u8]) -> Option<Entity> {
    let kind = match t {
        ty::LIST_LIGHT => Kind::Light,
        ty::LIST_SWITCH => Kind::Switch,
        ty::LIST_BINARY_SENSOR => Kind::BinarySensor,
        ty::LIST_SERVICES => Kind::Service,
        12..=18 | 43..=200 if t != ty::LIST_DONE => Kind::Other,
        _ => return None,
    };
    let mut e = Entity { kind, key: 0, object_id: String::new(), name: String::new(), effects: Vec::new(), args: Vec::new() };
    for (f, v) in fields(data) {
        match (kind, f) {
            (Kind::Service, 1) => e.name = v.text(),
            (Kind::Service, 2) => e.key = v.u32(),
            (Kind::Service, 3) => {
                if let Field::Bytes(b) = v {
                    let mut name = String::new();
                    let mut typ = 0;
                    for (af, av) in fields(b) {
                        match af {
                            1 => name = av.text(),
                            2 => typ = av.u32(),
                            _ => {}
                        }
                    }
                    e.args.push((name, typ));
                }
            }
            (Kind::Service, _) => {}
            (_, 1) => e.object_id = v.text(),
            (_, 2) => e.key = v.u32(),
            (_, 3) => e.name = v.text(),
            (Kind::Light, 11) if e.effects.len() < 64 => e.effects.push(v.text()),
            _ => {}
        }
    }
    Some(e)
}

/// A state message; `None` for other message types.
pub fn decode_state(t: u16, data: &[u8]) -> Option<State> {
    match t {
        ty::LIGHT_STATE => {
            let (mut key, mut on, mut brightness, mut rgb, mut effect) = (0, false, 0.0, [0.0f32; 3], String::new());
            for (f, v) in fields(data) {
                match f {
                    1 => key = v.u32(),
                    2 => on = v.boolean(),
                    3 => brightness = v.float(),
                    4 => rgb[0] = v.float(),
                    5 => rgb[1] = v.float(),
                    6 => rgb[2] = v.float(),
                    9 => effect = v.text(),
                    _ => {}
                }
            }
            Some(State::Light { key, on, brightness, rgb, effect })
        }
        ty::SWITCH_STATE => {
            let (mut key, mut on) = (0, false);
            for (f, v) in fields(data) {
                match f {
                    1 => key = v.u32(),
                    2 => on = v.boolean(),
                    _ => {}
                }
            }
            Some(State::Switch { key, on })
        }
        ty::BINARY_SENSOR_STATE => {
            let (mut key, mut on, mut missing) = (0, false, false);
            for (f, v) in fields(data) {
                match f {
                    1 => key = v.u32(),
                    2 => on = v.boolean(),
                    3 => missing = v.boolean(),
                    _ => {}
                }
            }
            Some(State::BinarySensor { key, on, missing })
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// the encrypted connection

fn noise_err(e: snow::Error) -> Error {
    Error::Unavailable(format!("ESPHome encryption: {e}"))
}

fn io_err(e: std::io::Error) -> Error {
    Error::Unavailable(format!("ESPHome connection: {e}"))
}

async fn read_frame(r: &mut OwnedReadHalf) -> Result<Vec<u8>> {
    let mut head = [0u8; 3];
    r.read_exact(&mut head).await.map_err(io_err)?;
    if head[0] != 0x01 {
        return Err(Error::Unavailable("ESPHome answered without encryption; check the API key".into()));
    }
    let len = usize::from(u16::from_be_bytes([head[1], head[2]]));
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await.map_err(io_err)?;
    Ok(buf)
}

fn frame(payload: &[u8]) -> Result<Vec<u8>> {
    let len = u16::try_from(payload.len()).map_err(|_| Error::internal("ESPHome frame too long"))?;
    let mut out = Vec::with_capacity(payload.len() + 3);
    out.push(0x01);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(payload);
    Ok(out)
}

/// An open, encrypted connection.
pub struct Conn {
    reader: OwnedReadHalf,
    writer: OwnedWriteHalf,
    noise: snow::TransportState,
    pub server_name: String,
}

impl Conn {
    /// Connect and do the handshake. `key` is the 32-byte API key.
    pub async fn open(addr: &str, key: &[u8; 32]) -> Result<Self> {
        let stream = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(addr))
            .await
            .map_err(|_| Error::Unavailable("ESPHome did not answer".into()))?
            .map_err(io_err)?;
        let _ = stream.set_nodelay(true);
        let (mut reader, mut writer) = stream.into_split();
        let params: snow::params::NoiseParams = NOISE.parse().map_err(noise_err)?;
        let mut hs = snow::Builder::new(params).psk(0, key).map_err(noise_err)?.prologue(PROLOGUE).map_err(noise_err)?.build_initiator().map_err(noise_err)?;
        let mut msg = [0u8; 96];
        let n = hs.write_message(&[], &mut msg).map_err(noise_err)?;
        let mut first = vec![0x01, 0x00, 0x00];
        let mut hand = Vec::with_capacity(n + 1);
        hand.push(0x00);
        hand.extend_from_slice(msg.get(..n).unwrap_or_default());
        first.extend_from_slice(&frame(&hand)?);
        writer.write_all(&first).await.map_err(io_err)?;
        let work = async {
            let hello = read_frame(&mut reader).await?;
            if hello.first() != Some(&0x01) {
                return Err(Error::Unavailable("ESPHome offered an unknown protocol".into()));
            }
            let server_name = hello.get(1..).unwrap_or_default().split(|b| *b == 0).next().map(|n| String::from_utf8_lossy(n).into_owned()).unwrap_or_default();
            let reply = read_frame(&mut reader).await?;
            match reply.split_first() {
                Some((0, rest)) => {
                    let mut out = [0u8; 96];
                    hs.read_message(rest, &mut out).map_err(|_| Error::Unavailable("ESPHome refused the API key".into()))?;
                }
                Some((_, rest)) => return Err(Error::Unavailable(format!("ESPHome refused the connection: {}", String::from_utf8_lossy(rest)))),
                None => return Err(Error::Unavailable("ESPHome closed the handshake".into())),
            }
            Ok(server_name)
        };
        let server_name = tokio::time::timeout(Duration::from_secs(10), work).await.map_err(|_| Error::Unavailable("ESPHome handshake timed out".into()))??;
        let noise = hs.into_transport_mode().map_err(noise_err)?;
        Ok(Self { reader, writer, noise, server_name })
    }

    pub async fn send(&mut self, t: u16, data: &[u8]) -> Result<()> {
        let len = u16::try_from(data.len()).map_err(|_| Error::internal("ESPHome message too long"))?;
        let mut plain = Vec::with_capacity(data.len() + 4);
        plain.extend_from_slice(&t.to_be_bytes());
        plain.extend_from_slice(&len.to_be_bytes());
        plain.extend_from_slice(data);
        let mut out = vec![0u8; plain.len() + 16];
        let n = self.noise.write_message(&plain, &mut out).map_err(noise_err)?;
        let f = frame(out.get(..n).unwrap_or_default())?;
        self.writer.write_all(&f).await.map_err(io_err)
    }

    fn open_message(&mut self, cipher: &[u8]) -> Result<(u16, Vec<u8>)> {
        let mut plain = vec![0u8; cipher.len()];
        let n = self.noise.read_message(cipher, &mut plain).map_err(noise_err)?;
        let plain = plain.get(..n).unwrap_or_default();
        let (head, data) = plain.split_first_chunk::<4>().ok_or_else(|| Error::Unavailable("ESPHome sent a short message".into()))?;
        let t = u16::from_be_bytes([head[0], head[1]]);
        Ok((t, data.to_vec()))
    }

    /// Wait for the next message (no cancellation: use only outside `select!`).
    pub async fn recv(&mut self) -> Result<(u16, Vec<u8>)> {
        let cipher = read_frame(&mut self.reader).await?;
        self.open_message(&cipher)
    }

    /// Answer the server's own requests. Returns false for a disconnect.
    async fn answer(&mut self, t: u16) -> Result<bool> {
        match t {
            ty::PING_REQ => self.send(ty::PING_RESP, &[]).await?,
            ty::GET_TIME_REQ => {
                let now = u32::try_from(smhub_model::time::now_unix()).unwrap_or(0);
                let mut p = Pb::default();
                p.fixed32(1, now);
                self.send(ty::GET_TIME_RESP, &p.0).await?;
            }
            ty::DISCONNECT_REQ => {
                let _ = self.send(ty::DISCONNECT_RESP, &[]).await;
                return Ok(false);
            }
            _ => {}
        }
        Ok(true)
    }

    /// Say hello, sign in (no password), and list the entities.
    pub async fn start(&mut self) -> Result<(String, Vec<Entity>)> {
        self.send(ty::HELLO_REQ, &hello().0).await?;
        let work = async {
            let mut name = String::new();
            loop {
                let (t, d) = self.recv().await?;
                if t == ty::HELLO_RESP {
                    for (f, v) in fields(&d) {
                        if f == 4 {
                            name = v.text();
                        }
                    }
                    break;
                }
                self.answer(t).await?;
            }
            // servers before ESPHome 2026.1 sign in here (no password); newer ones sign in on
            // hello and ignore this message
            self.send(ty::CONNECT_REQ, &[]).await?;
            self.send(ty::LIST_ENTITIES_REQ, &[]).await?;
            let mut list = Vec::new();
            loop {
                let (t, d) = self.recv().await?;
                match t {
                    ty::LIST_DONE => break,
                    ty::CONNECT_RESP | ty::DEVICE_INFO_RESP => {}
                    _ => {
                        if let Some(e) = decode_entity(t, &d) {
                            if list.len() < MAX_ENTITIES {
                                list.push(e);
                            }
                        } else if !self.answer(t).await? {
                            return Err(Error::Unavailable("ESPHome closed the connection".into()));
                        }
                    }
                }
            }
            Ok::<_, Error>((name, list))
        };
        let (name, list) = tokio::time::timeout(Duration::from_secs(20), work).await.map_err(|_| Error::Unavailable("ESPHome did not list its entities".into()))??;
        self.send(ty::SUBSCRIBE_STATES, &[]).await?;
        Ok((name, list))
    }

    pub fn split(self) -> (OwnedReadHalf, Session) {
        (self.reader, Session { writer: self.writer, noise: self.noise })
    }
}

/// The writing side and the cipher, after the reading side went to its own task.
pub struct Session {
    writer: OwnedWriteHalf,
    noise: snow::TransportState,
}

impl Session {
    async fn send(&mut self, t: u16, data: &[u8]) -> Result<()> {
        let len = u16::try_from(data.len()).map_err(|_| Error::internal("ESPHome message too long"))?;
        let mut plain = Vec::with_capacity(data.len() + 4);
        plain.extend_from_slice(&t.to_be_bytes());
        plain.extend_from_slice(&len.to_be_bytes());
        plain.extend_from_slice(data);
        let mut out = vec![0u8; plain.len() + 16];
        let n = self.noise.write_message(&plain, &mut out).map_err(noise_err)?;
        let f = frame(out.get(..n).unwrap_or_default())?;
        self.writer.write_all(&f).await.map_err(io_err)
    }

    fn open_message(&mut self, cipher: &[u8]) -> Result<(u16, Vec<u8>)> {
        let mut plain = vec![0u8; cipher.len()];
        let n = self.noise.read_message(cipher, &mut plain).map_err(noise_err)?;
        let plain = plain.get(..n).unwrap_or_default();
        let (head, data) = plain.split_first_chunk::<4>().ok_or_else(|| Error::Unavailable("ESPHome sent a short message".into()))?;
        Ok((u16::from_be_bytes([head[0], head[1]]), data.to_vec()))
    }
}

/// One connection's life: connect, list, then pass states up and commands down until it ends.
async fn session(addr: &str, key: &[u8; 32], commands: &mut mpsc::Receiver<Command>, events: &mpsc::Sender<Event>) -> Result<()> {
    let mut conn = Conn::open(addr, key).await?;
    let (name, entities) = conn.start().await?;
    let server = conn.server_name.clone();
    let _ = events.send(Event::Connected { name, server }).await;
    let _ = events.send(Event::Entities(entities)).await;
    let (mut reader, mut sess) = conn.split();
    let (ftx, mut frx) = mpsc::channel::<Result<Vec<u8>>>(16);
    let read_task = tokio::spawn(async move {
        loop {
            let f = read_frame(&mut reader).await;
            let end = f.is_err();
            if ftx.send(f).await.is_err() || end {
                break;
            }
        }
    });
    // the last frame from the server and the last ping of ours; commands sent do not count
    let mut heard = tokio::time::Instant::now();
    let mut pinged = heard;
    let result = loop {
        tokio::select! {
            f = tokio::time::timeout_at(heard.max(pinged) + QUIET, frx.recv()) => {
                let cipher = match f {
                    Err(_) if heard.elapsed() >= SILENCE => break Err(Error::Unavailable("ESPHome went silent".into())),
                    Err(_) => {
                        if let Err(e) = sess.send(ty::PING_REQ, &[]).await {
                            break Err(e);
                        }
                        pinged = tokio::time::Instant::now();
                        continue;
                    }
                    Ok(None) => break Err(Error::Unavailable("ESPHome closed the connection".into())),
                    Ok(Some(Err(e))) => break Err(e),
                    Ok(Some(Ok(c))) => c,
                };
                heard = tokio::time::Instant::now();
                let (t, d) = match sess.open_message(&cipher) {
                    Ok(m) => m,
                    Err(e) => break Err(e),
                };
                if let Some(s) = decode_state(t, &d) {
                    if events.send(Event::State(s)).await.is_err() {
                        break Ok(());
                    }
                    continue;
                }
                let r = match t {
                    ty::PING_REQ => sess.send(ty::PING_RESP, &[]).await,
                    ty::GET_TIME_REQ => {
                        let mut p = Pb::default();
                        p.fixed32(1, u32::try_from(smhub_model::time::now_unix()).unwrap_or(0));
                        sess.send(ty::GET_TIME_RESP, &p.0).await
                    }
                    ty::DISCONNECT_REQ => {
                        let _ = sess.send(ty::DISCONNECT_RESP, &[]).await;
                        break Err(Error::Unavailable("ESPHome ended the connection".into()));
                    }
                    _ => Ok(()),
                };
                if let Err(e) = r {
                    break Err(e);
                }
            }
            c = commands.recv() => {
                let Some(c) = c else {
                    let _ = sess.send(ty::DISCONNECT_REQ, &[]).await;
                    break Ok(());
                };
                let (t, p) = encode(&c);
                if let Err(e) = sess.send(t, &p.0).await {
                    break Err(e);
                }
            }
        }
    };
    read_task.abort();
    result
}

/// Keep a connection to ESPHome until the command channel closes, reconnecting with a growing
/// pause. Every loss is reported as `Disconnected`.
pub async fn run(addr: String, key: [u8; 32], mut commands: mpsc::Receiver<Command>, events: mpsc::Sender<Event>) {
    let pauses = [1u64, 2, 5, 10, 30, 60];
    let mut attempt = 0usize;
    loop {
        let started = std::time::Instant::now();
        match session(&addr, &key, &mut commands, &events).await {
            Ok(()) => return,
            Err(e) => {
                if events.send(Event::Disconnected(e.message().to_owned())).await.is_err() {
                    return;
                }
            }
        }
        if started.elapsed() > Duration::from_secs(120) {
            attempt = 0;
        }
        let pause = Duration::from_secs(pauses.get(attempt).copied().unwrap_or(60));
        attempt += 1;
        // commands that arrive while disconnected are dropped (the hub reports "not answering")
        let deadline = tokio::time::sleep(pause);
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                () = &mut deadline => break,
                c = commands.recv() => if c.is_none() { return },
            }
        }
    }
}

/// The API key from its base64 form.
pub fn key_from_b64(s: &str) -> Option<[u8; 32]> {
    use base64::Engine;
    let v = base64::engine::general_purpose::STANDARD.decode(s.trim()).ok()?;
    <[u8; 32]>::try_from(v.as_slice()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protobuf_round_trip() {
        let c = Command::Light(LightCommand { key: 0xdead_beef, on: Some(true), brightness: Some(0.5), rgb: Some([1.0, 0.0, 0.25]), effect: Some("Rainbow".into()), transition_ms: Some(300), flash_ms: None });
        let (t, p) = encode(&c);
        assert_eq!(t, ty::LIGHT_COMMAND);
        let got: Vec<(u32, u64)> = fields(&p.0).map(|(f, v)| (f, v.u64())).collect();
        assert_eq!(got.first(), Some(&(1, 0xdead_beef)));
        assert!(got.contains(&(3, 1)));
        let effect: Vec<String> = fields(&p.0).filter(|(f, _)| *f == 19).map(|(_, v)| v.text()).collect();
        assert_eq!(effect, vec!["Rainbow".to_owned()]);
        let mut s = Pb::default();
        s.fixed32(1, 7).boolean(2, true).float(3, 0.75).float(4, 1.0).string(9, "Pulse");
        assert_eq!(decode_state(ty::LIGHT_STATE, &s.0), Some(State::Light { key: 7, on: true, brightness: 0.75, rgb: [1.0, 0.0, 0.0], effect: "Pulse".into() }));
    }

    #[test]
    fn entities() {
        let mut l = Pb::default();
        l.string(1, "ambilight").fixed32(2, 42).string(3, "AmbiLight").string(11, "Pulse").string(11, "Rainbow");
        let e = decode_entity(ty::LIST_LIGHT, &l.0);
        assert!(e.as_ref().is_some_and(|e| e.kind == Kind::Light && e.key == 42 && e.effects.len() == 2 && e.object_id == "ambilight"));
        let mut arg = Pb::default();
        arg.string(1, "song").uint(2, 3);
        let mut s = Pb::default();
        s.string(1, "play_rtttl").fixed32(2, 9).bytes(3, &arg.0);
        let e = decode_entity(ty::LIST_SERVICES, &s.0);
        assert!(e.is_some_and(|e| e.kind == Kind::Service && e.args == vec![("song".to_owned(), 3)]));
        assert!(decode_entity(ty::LIST_DONE, &[]).is_none());
    }

    #[test]
    fn malformed_input_stops() {
        assert_eq!(fields(&[0x0a, 0x05, 0x01]).count(), 0);
        assert_eq!(fields(&[0xff, 0xff]).count(), 0);
        assert!(key_from_b64("VGVzdGluZ1RSTkdFbmNyeXB0aW9uS2V5MTIzNDU2Nzg=").is_some());
        assert!(key_from_b64("short").is_none());
    }
}
