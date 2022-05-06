//! Provides a type representing a Redis protocol frame as well as utilities for
//! parsing frames from a byte array.

use std::{fmt, io};
use std::collections::HashMap;
use std::convert::TryFrom;
use std::io::Cursor;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::num::TryFromIntError;
use std::string::FromUtf8Error;

use bytes::Buf;
use num_enum::TryFromPrimitive;

const U32_LENGTH: usize = std::mem::size_of::<u32>();

/// A frame in the SPOP protocol.
#[derive(Clone, Debug)]
pub enum Frame {
    HAProxyHello { header: FrameHeader, content: HashMap<String, TypedData> },
    HAProxyDisconnect { header: FrameHeader },
    Notify { header: FrameHeader },
    AgentHello { header: FrameHeader, content: HashMap<String, TypedData> },
    AgentDisconnect { header: FrameHeader },
    Ack { header: FrameHeader },
}

#[allow(non_camel_case_types)]
#[derive(TryFromPrimitive, Clone, PartialEq, Debug)]
#[repr(u8)]
pub enum FrameType {
    UNSET = 0,
    HAPROXY_HELLO = 1,
    HAPROXY_DISCONNECT = 2,
    NOTIFY = 3,
    AGENT_HELLO = 101,
    AGENT_DISCONNECT = 102,
    ACK = 103,
}

#[derive(Clone, Debug)]
pub struct FrameHeader {
    pub r#type: FrameType,
    pub flags: FrameFlags,
    pub stream_id: u64,
    pub frame_id: u64,
}

#[derive(Clone, Debug)]
pub struct FrameFlags(u32);

impl FrameFlags {
    pub fn is_fin(&self) -> bool {
        self.0 & 0x00000001u32 != 0
    }
    pub fn is_abort(&self) -> bool {
        self.0 & 0x00000002u32 != 0
    }

    pub fn new(is_fin: bool, is_abort: bool) -> Self {
        let mut val = 0_u32;

        if is_fin {
            val |= 0x00000001u32;
        } else {
            val &= !0x00000001u32;
        }

        if is_abort {
            val |= 0x00000002u32;
        } else {
            val &= !0x00000002u32;
        }

        Self(val)
    }

    pub fn val(&self) -> u32 {
        self.0
    }
}

#[derive(PartialEq, Clone, Debug)]
pub enum TypedData {
    NULL,
    BOOL(bool),
    INT32(i32),
    UINT32(u32),
    INT64(i64),
    UINT64(u64),
    IPV4(Ipv4Addr),
    IPV6(Ipv6Addr),
    STRING(String),
    BINARY(Vec<u8>),
}

#[derive(TryFromPrimitive, PartialEq, Debug)]
#[repr(u8)]
pub enum TypedDataType {
    NULL = 0,
    BOOL = 1,
    INT32 = 2,
    UINT32 = 3,
    INT64 = 4,
    UINT64 = 5,
    IPV4 = 6,
    IPV6 = 7,
    STRING = 8,
    BINARY = 9,
}

#[derive(Debug)]
pub enum VarintError {
    InsufficientBytes,
}

#[derive(Debug)]
pub enum FrameHeaderError {
    InsufficientBytes,
    InvalidFrameType(u8),
    InvalidStreamId(VarintError),
    InvalidFrameId(VarintError),
}

#[derive(Debug)]
pub enum StringError {
    InsufficientBytes,
    InvalidSize(VarintError),
    Utf8Error(String),
}


#[derive(Debug)]
pub enum Ipv4Error {
    InsufficientBytes,
}

#[derive(Debug)]
pub enum Ipv6Error {
    InsufficientBytes,
}

#[derive(Debug)]
pub enum TypedDataError {
    InsufficientBytes,
    InvalidType(u8),
    InvalidString(StringError),
    NumberConversionError(TypedDataType, u64),
    NumberParsingError(TypedDataType, VarintError),
    InvalidIpv4(Ipv4Error),
    InvalidIpv6(Ipv6Error),
    NotSupported,
}

#[derive(Debug)]
pub enum KVListError {
    InsufficientBytes,
    InvalidKVListName(StringError),
    InvalidKVListValue(TypedDataError),
}

#[derive(Debug)]
pub enum FramePayloadError {
    InsufficientBytes,
    InvalidKVList(KVListError),
    NotSupported,
}

#[derive(Debug)]
pub enum FrameError {
    InsufficientBytes,
    InvalidFrameHeader(FrameHeaderError),
    InvalidFramePayload(FramePayloadError),
}

#[derive(Debug)]
pub enum Error {
    /// Not enough data is available to parse a message
    Incomplete,

    /// Parsing error
    InvalidCursor { expected: usize, remaining: usize },

    /// Only full payload os supported for now
    FragmentedModeNotSupported,

    ///
    NotSupported,

    /// Invalid message encoding
    InvalidFrame(FrameError),

    ///
    IO(io::Error),
    Other(String),

    ///
    None,
}

impl Frame {
    /// Checks if an entire message can be decoded from `src`
    /// It must "consume" the cursor, in order to give to the caller
    /// the length of the frame.
    pub fn check(src: &mut Cursor<&[u8]>) -> Result<(), Error> {
        if src.remaining() < U32_LENGTH {
            return Err(Error::Incomplete);
        }
        let len = src.get_u32() as usize;
        if src.remaining() < len {
            return Err(Error::Incomplete);
        }
        src.advance(len);
        // still there ?
        return Ok(());
    }

    /// The message has already been validated with `check`.
    pub fn parse(src: &mut Cursor<&[u8]>) -> Result<Frame, Error> {
        let len = src.get_u32() as usize;
        if len != src.remaining() {
            return Err(Error::InvalidCursor { expected: len, remaining: src.remaining() });
        }
        let frame_header: FrameHeader = parse_frame_header(src).map_err(|e| Error::InvalidFrame(FrameError::InvalidFrameHeader(e)))?;

        if !frame_header.flags.is_fin() {
            return Err(Error::FragmentedModeNotSupported);
        }

        parse_frame_payload(src, &frame_header).map_err(|e| Error::InvalidFrame(FrameError::InvalidFramePayload(e)))
    }

    pub fn write_to(&self, dst: &mut Cursor<&[u8]>) -> Result<(), Error> {
        Ok(())
    }
}

pub fn parse_frame_payload(src: &mut Cursor<&[u8]>, frame_header: &FrameHeader) -> Result<Frame, FramePayloadError> {
    match frame_header.r#type {
        FrameType::HAPROXY_HELLO => {
            let body = parse_kv_list(src).map_err(|err| FramePayloadError::InvalidKVList(err))?;
            Ok(Frame::HAProxyHello {
                header: frame_header.to_owned(),
                content: body,
            })
        }
        _ => Err(FramePayloadError::NotSupported)
    }
}

pub fn parse_kv_list(src: &mut Cursor<&[u8]>) -> Result<HashMap::<String, TypedData>, KVListError> {
    let mut body = HashMap::<String, TypedData>::new();
    while src.has_remaining() {
        let name = parse_string(src).map_err(|e| KVListError::InvalidKVListName(e))?;
        let value = parse_typed_data(src).map_err(|e| KVListError::InvalidKVListValue(e))?;
        body.insert(name, value);
    }
    Ok(body)
}

pub fn parse_typed_data(src: &mut Cursor<&[u8]>) -> Result<TypedData, TypedDataError> {
    let raw = src.get_u8();
    let r#type: TypedDataType = TypedDataType::try_from(raw & 0x0F_u8).map_err(|_| TypedDataError::InvalidType(raw))?;
    let value = match r#type {
        TypedDataType::NULL => TypedData::NULL,
        TypedDataType::BOOL => TypedData::BOOL(raw & 0x10_u8 == 0x10_u8),
        TypedDataType::INT32 => {
            let raw = parse_varint(src).map_err(|e| TypedDataError::NumberParsingError(TypedDataType::INT32, e))?;
            let value = i32::try_from(raw).map_err(|_| TypedDataError::NumberConversionError(TypedDataType::INT32, raw))?;
            TypedData::INT32(value)
        }
        TypedDataType::UINT32 => {
            let raw = parse_varint(src).map_err(|e| TypedDataError::NumberParsingError(TypedDataType::UINT32, e))?;
            let value = u32::try_from(raw).map_err(|_| TypedDataError::NumberConversionError(TypedDataType::UINT32, raw))?;
            TypedData::UINT32(value)
        }
        TypedDataType::INT64 => {
            let raw = parse_varint(src).map_err(|e| TypedDataError::NumberParsingError(TypedDataType::INT64, e))?;
            let value = raw as i64;
            TypedData::INT64(value)
        }
        TypedDataType::UINT64 => {
            let raw = parse_varint(src).map_err(|e| TypedDataError::NumberParsingError(TypedDataType::UINT64, e))?;
            TypedData::UINT64(raw)
        }
        TypedDataType::IPV4 => {
            if src.remaining() < 4 {
                return Err(TypedDataError::InvalidIpv4(Ipv4Error::InsufficientBytes));
            }
            let val = Ipv4Addr::new(src.get_u8(), src.get_u8(), src.get_u8(), src.get_u8());
            TypedData::IPV4(val)
        }
        TypedDataType::IPV6 => {
            if src.remaining() < 16 {
                return Err(TypedDataError::InvalidIpv6(Ipv6Error::InsufficientBytes));
            }
            let val = Ipv6Addr::from([
                src.get_u8(), //0
                src.get_u8(), //1
                src.get_u8(), //2
                src.get_u8(), //3
                src.get_u8(), //4
                src.get_u8(), //5
                src.get_u8(), //6
                src.get_u8(), //7
                src.get_u8(), //8
                src.get_u8(), //9
                src.get_u8(), //10
                src.get_u8(), //11
                src.get_u8(), //12
                src.get_u8(), //13
                src.get_u8(), //14
                src.get_u8(), //15
            ]);
            TypedData::IPV6(val)
        }
        TypedDataType::STRING => {
            let value = parse_string(src).map_err(|err| TypedDataError::InvalidString(err))?;
            TypedData::STRING(value)
        }
        TypedDataType::BINARY => {
            return Err(TypedDataError::NotSupported)
        }
    };

    Ok(value)
}

pub fn parse_string(src: &mut Cursor<&[u8]>) -> Result<String, StringError> {
    let len = parse_varint(src).map_err(|e| StringError::InvalidSize(e))?;
    let val = if len == 0 {
        "".to_string()
    } else {
        let str_len = len as usize;
        if str_len > src.remaining() {
            return Err(StringError::InsufficientBytes);
        }
        let bytes = src.copy_to_bytes(str_len);
        std::str::from_utf8(&bytes[..]).map_err(|e| StringError::Utf8Error(e.to_string())).unwrap().to_string()
    };
    Ok(val)
}

pub fn parse_frame_header(src: &mut Cursor<&[u8]>) -> Result<FrameHeader, FrameHeaderError> {
    let raw = src.get_u8();
    let r#type = FrameType::try_from(raw).map_err(|_| FrameHeaderError::InvalidFrameType(raw))?;
    let raw = src.get_u32();
    let flags = FrameFlags(raw);

    let stream_id = parse_varint(src).map_err(|e| FrameHeaderError::InvalidStreamId(e))?;
    let frame_id = parse_varint(src).map_err(|e| FrameHeaderError::InvalidFrameId(e))?;
    Ok(FrameHeader {
        r#type,
        flags,
        stream_id,
        frame_id,
    })
}

pub fn parse_varint(src: &mut Cursor<&[u8]>) -> Result<u64, VarintError> {
    if src.remaining() < 1 {
        return Err(VarintError::InsufficientBytes);
    }

    let mut res = src.get_u8() as u64;
    if res >= 240 {
        let mut bit_offset: u8 = 4;
        loop {
            if src.remaining() < 1 {
                return Err(VarintError::InsufficientBytes);
            }
            let b = src.get_u8();
            res += (b as u64) << bit_offset;
            bit_offset += 7;
            if b < 128 {
                break;
            }
        }
    }
    Ok(res)
}

impl From<String> for Error {
    fn from(src: String) -> Error {
        Error::Other(src.into())
    }
}

impl From<&str> for Error {
    fn from(src: &str) -> Error {
        src.to_string().into()
    }
}

impl From<FromUtf8Error> for Error {
    fn from(_src: FromUtf8Error) -> Error {
        "protocol error; invalid frame format".into()
    }
}

impl From<TryFromIntError> for Error {
    fn from(_src: TryFromIntError) -> Error {
        "protocol error; invalid frame format".into()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Incomplete => write!(f, "stream ended early"),
            Error::InvalidCursor { remaining, expected } => write!(f, "InvalidCursor expected: {}, remaining: {}", expected, remaining),
            Error::FragmentedModeNotSupported => write!(f, "FragmentedModeNotSupported"),
            Error::NotSupported => write!(f, "NotSupported"),
            Error::InvalidFrame(err) => write!(f, "InvalidFrame {}", err),
            Error::None => write!(f, "<none>"),
            Error::IO(err) => err.fmt(f),
            Error::Other(err) => err.fmt(f),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::IO(err)
    }
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            FrameError::InsufficientBytes => write!(f, "FrameError::InsufficientBytes"),
            FrameError::InvalidFrameHeader(err) => write!(f, "InvalidFrameHeader {}", err),
            FrameError::InvalidFramePayload(err) => write!(f, "InvalidFramePayload {}", err),
        }
    }
}


impl fmt::Display for VarintError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            VarintError::InsufficientBytes => write!(f, "VarintError::InsufficientBytes"),
        }
    }
}

impl fmt::Display for FrameHeaderError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            FrameHeaderError::InsufficientBytes => write!(f, "FrameHeaderError::InsufficientBytes"),
            FrameHeaderError::InvalidFrameType(r#type) => write!(f, "InvalidFrameType {}", r#type),
            FrameHeaderError::InvalidStreamId(err) => write!(f, "InvalidStreamId {}", err),
            FrameHeaderError::InvalidFrameId(err) => write!(f, "InvalidStreamId {}", err),
        }
    }
}

impl fmt::Display for FramePayloadError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            FramePayloadError::InsufficientBytes => write!(f, "FrameHeaderError::InsufficientBytes"),
            FramePayloadError::InvalidKVList(err) => write!(f, "InvalidKVList {}", err),
            FramePayloadError::NotSupported => write!(f, "FramePayloadError::NotSupported"),
        }
    }
}

impl fmt::Display for KVListError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            KVListError::InsufficientBytes => write!(f, "KVListError::InsufficientBytes"),
            KVListError::InvalidKVListName(err) => write!(f, "InvalidKVListName {}", err),
            KVListError::InvalidKVListValue(err) => write!(f, "InvalidKVListValue {}", err),
        }
    }
}

impl fmt::Display for StringError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            StringError::InsufficientBytes => write!(f, "StringError::InsufficientBytes"),
            StringError::InvalidSize(err) => write!(f, "StringError::InvalidSize {}", err),
            StringError::Utf8Error(err) => write!(f, "StringError::Utf8Error {}", err),
        }
    }
}

impl fmt::Display for TypedDataError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            TypedDataError::InsufficientBytes => write!(f, "TypedDataError::InsufficientBytes"),
            TypedDataError::InvalidType(r#type) => write!(f, "TypedDataError::InvalidType {}", r#type),
            TypedDataError::InvalidString(err) => write!(f, "TypedDataError::InvalidString {}", err),
            TypedDataError::InvalidIpv4(err) => write!(f, "TypedDataError::InvalidIpv4 {}", err),
            TypedDataError::InvalidIpv6(err) => write!(f, "TypedDataError::InvalidIpv6 {}", err),
            TypedDataError::NotSupported => write!(f, "TypedDataError::NotSupported"),
            TypedDataError::NumberConversionError(data_type, r#u64) => write!(f, "TypedDataError::NumberConversionError ({}, {})", data_type, r#u64),
            TypedDataError::NumberParsingError(data_type, err) => write!(f, "TypedDataError::InvalidIpv6 ({}, {})", data_type, err),
        }
    }
}

impl fmt::Display for TypedDataType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            TypedDataType::NULL => write!(f, "NULL"),
            TypedDataType::BOOL => write!(f, "BOOL"),
            TypedDataType::INT32 => write!(f, "INT32"),
            TypedDataType::UINT32 => write!(f, "UINT32"),
            TypedDataType::INT64 => write!(f, "INT64"),
            TypedDataType::UINT64 => write!(f, "UINT64"),
            TypedDataType::IPV4 => write!(f, "IPV4"),
            TypedDataType::IPV6 => write!(f, "IPV6"),
            TypedDataType::STRING => write!(f, "STRING"),
            TypedDataType::BINARY => write!(f, "BINARY"),
        }
    }
}

impl fmt::Display for Ipv4Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Ipv4Error::InsufficientBytes => write!(f, "Ipv4Error::InsufficientBytes"),
        }
    }
}

impl fmt::Display for Ipv6Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Ipv6Error::InsufficientBytes => write!(f, "Ipv4Error::InsufficientBytes"),
        }
    }
}
