//! Provides a type representing a Redis protocol frame as well as utilities for
//! parsing frames from a byte array.

use std::collections::HashMap;
use std::convert::TryFrom;
use std::io::Cursor;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::num::TryFromIntError;
use std::string::FromUtf8Error;
use std::{fmt, io};

use bytes::{Buf, BufMut, BytesMut};
use num_enum::{IntoPrimitive, TryFromPrimitive};

const U32_LENGTH: usize = std::mem::size_of::<u32>();

/// A frame in the SPOP protocol.
#[derive(Clone, Debug)]
pub enum Frame {
    HAProxyHello {
        header: FrameHeader,
        content: HashMap<String, TypedData>,
    },
    HAProxyDisconnect {
        header: FrameHeader,
        content: HashMap<String, TypedData>,
    },
    Notify {
        header: FrameHeader,
        messages: HashMap<String, HashMap<String, TypedData>>,
    },
    AgentHello {
        header: FrameHeader,
        content: HashMap<String, TypedData>,
    },
    AgentDisconnect {
        header: FrameHeader,
    },
    Ack {
        header: FrameHeader,
        actions: Vec<Action>,
    },
}

#[derive(IntoPrimitive, TryFromPrimitive, PartialEq, Eq, Clone, Debug)]
#[repr(u8)]
#[allow(non_camel_case_types)]
pub enum ActionVarScope {
    PROCESS = 0,
    SESSION = 1,
    TRANSACTION = 2,
    REQUEST = 3,
    RESPONSE = 4,
}

#[allow(non_camel_case_types)]
#[derive(TryFromPrimitive, IntoPrimitive, Copy, Clone, PartialEq, Debug)]
#[repr(u8)]
pub enum ActionType {
    SET_VAR = 1,
    UNSET_VAR = 2,
}

#[derive(Clone, Debug)]
#[allow(non_camel_case_types)]
pub enum Action {
    SetVar {
        scope: ActionVarScope,
        name: String,
        value: TypedData,
    },
    UnsetVar {
        scope: ActionVarScope,
        name: String,
    },
}

#[allow(non_camel_case_types)]
#[derive(TryFromPrimitive, IntoPrimitive, Copy, Clone, PartialEq, Debug)]
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

impl FrameType {
    pub fn write_to(self, dst: &mut BytesMut) -> Result<(), Error> {
        dst.put_u8(self.into());
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct FrameHeader {
    pub r#type: FrameType,
    pub flags: FrameFlags,
    pub stream_id: u64,
    pub frame_id: u64,
}

impl FrameHeader {
    pub fn reply_header(&self, frame_type: &FrameType) -> FrameHeader {
        FrameHeader {
            frame_id: self.frame_id,
            stream_id: self.stream_id,
            flags: FrameFlags::new(true, false),
            r#type: frame_type.to_owned(),
        }
    }
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

#[derive(TryFromPrimitive, IntoPrimitive, PartialEq, Debug)]
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
pub enum ActionError {
    InsufficientBytes,
    InvalidActionType(u8),
    InvalidActionScope(u8),
    InvalidNumberOfArgs(ActionType, u8, u8),
    InvalidSetVarActionVarName(StringError),
    InvalidSetVarActionVarValue(TypedDataError),
    InvalidUnsetVarActionVarName(StringError),
}

#[derive(Debug)]
pub enum ListOfActionsError {
    InvalidAction(ActionError),
}

#[derive(Debug)]
pub enum ListOfMessagesError {
    InsufficientBytes,
    InvalidMessageName(StringError),
    InvalidKVListName(StringError),
    InvalidKVListValue(TypedDataError),
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
    InvalidListOfMessages(ListOfMessagesError),
    InvalidListOfActions(ListOfActionsError),
    NotSupported(FrameType),
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
    InvalidCursor {
        expected: usize,
        remaining: usize,
    },

    /// Only full payload os supported for now
    FragmentedModeNotSupported,

    ///
    NotSupported,
    Disconnect,

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
            return Err(Error::InvalidCursor {
                expected: len,
                remaining: src.remaining(),
            });
        }
        let frame_header: FrameHeader = parse_frame_header(src)
            .map_err(|e| Error::InvalidFrame(FrameError::InvalidFrameHeader(e)))?;

        if !frame_header.flags.is_fin() {
            return Err(Error::FragmentedModeNotSupported);
        }

        parse_frame_payload(src, &frame_header)
            .map_err(|e| Error::InvalidFrame(FrameError::InvalidFramePayload(e)))
    }

    pub fn write_to(&self, dst: &mut BytesMut) -> Result<(), Error> {
        match &self {
            Frame::AgentHello { header, content } => {
                write_frame_header(dst, header).unwrap();
                write_kv_list(dst, content).unwrap();
                Ok(())
            }
            Frame::Ack { header, actions } => {
                write_frame_header(dst, header).unwrap();
                write_list_of_actions(dst, actions).unwrap();
                Ok(())
            }
            _ => Err(Error::NotSupported),
        }
    }

    pub fn frame_header(&self) -> &FrameHeader {
        match self {
            Frame::HAProxyHello { header, content: _ } => header,
            Frame::HAProxyDisconnect { header, content: _ } => header,
            Frame::Notify {
                header,
                messages: _,
            } => header,
            Frame::AgentHello { header, content: _ } => header,
            Frame::AgentDisconnect { header } => header,
            Frame::Ack { header, actions: _ } => header,
        }
    }
}

pub fn parse_frame_payload(
    src: &mut Cursor<&[u8]>,
    frame_header: &FrameHeader,
) -> Result<Frame, FramePayloadError> {
    match frame_header.r#type {
        FrameType::HAPROXY_HELLO => {
            let body = parse_kv_list(src).map_err(|err| FramePayloadError::InvalidKVList(err))?;
            Ok(Frame::HAProxyHello {
                header: frame_header.to_owned(),
                content: body,
            })
        }
        FrameType::AGENT_HELLO => {
            let body = parse_kv_list(src).map_err(|err| FramePayloadError::InvalidKVList(err))?;
            Ok(Frame::AgentHello {
                header: frame_header.to_owned(),
                content: body,
            })
        }
        FrameType::HAPROXY_DISCONNECT => {
            let body = parse_kv_list(src).map_err(|err| FramePayloadError::InvalidKVList(err))?;
            Ok(Frame::HAProxyDisconnect {
                header: frame_header.to_owned(),
                content: body,
            })
        }
        FrameType::NOTIFY => {
            let body = parse_list_of_messages(src)
                .map_err(|err| FramePayloadError::InvalidListOfMessages(err))?;
            Ok(Frame::Notify {
                header: frame_header.to_owned(),
                messages: body,
            })
        }
        FrameType::ACK => {
            let body = parse_list_of_actions(src)
                .map_err(|err| FramePayloadError::InvalidListOfActions(err))?;
            Ok(Frame::Ack {
                header: frame_header.to_owned(),
                actions: body,
            })
        }
        _ => Err(FramePayloadError::NotSupported(frame_header.r#type)),
    }
}

pub fn write_list_of_actions(dst: &mut BytesMut, actions: &Vec<Action>) -> Result<(), Error> {
    for action in actions {
        write_action(dst, &action).unwrap();
    }
    Ok(())
}

pub fn write_action(dst: &mut BytesMut, action: &Action) -> Result<(), Error> {
    match action {
        Action::SetVar { name, value, scope } => {
            dst.put_u8(ActionType::SET_VAR.into());
            dst.put_u8(3);
            dst.put_u8(scope.to_owned().into());
            write_string(dst, name).unwrap();
            write_typed_data(dst, value).unwrap();
        }
        Action::UnsetVar { name, scope } => {
            dst.put_u8(ActionType::UNSET_VAR.into());
            dst.put_u8(2);
            dst.put_u8(scope.to_owned().into());
            write_string(dst, name).unwrap();
        }
    }
    Ok(())
}

pub fn parse_list_of_actions(src: &mut Cursor<&[u8]>) -> Result<Vec<Action>, ListOfActionsError> {
    let mut actions: Vec<Action> = vec![];

    while src.has_remaining() {
        let action: Action =
            parse_action(src).map_err(|err| ListOfActionsError::InvalidAction(err))?;
        actions.push(action)
    }
    Ok(actions)
}

pub fn parse_action(src: &mut Cursor<&[u8]>) -> Result<Action, ActionError> {
    let r#type = parse_action_type(src)?;
    let nb_args = src.get_u8();
    match r#type {
        ActionType::SET_VAR => {
            if nb_args != 3 {
                Err(ActionError::InvalidNumberOfArgs(
                    ActionType::SET_VAR,
                    nb_args,
                    3,
                ))
            } else {
                let scope = parse_action_scope(src)?;
                let name =
                    parse_string(src).map_err(|e| ActionError::InvalidSetVarActionVarName(e))?;
                let value = parse_typed_data(src)
                    .map_err(|e| ActionError::InvalidSetVarActionVarValue(e))?;
                Ok(Action::SetVar { scope, name, value })
            }
        }
        ActionType::UNSET_VAR => {
            if nb_args != 2 {
                Err(ActionError::InvalidNumberOfArgs(
                    ActionType::SET_VAR,
                    nb_args,
                    2,
                ))
            } else {
                let scope = parse_action_scope(src)?;
                let name =
                    parse_string(src).map_err(|e| ActionError::InvalidUnsetVarActionVarName(e))?;
                Ok(Action::UnsetVar { scope, name })
            }
        }
    }
}

pub fn parse_action_type(src: &mut Cursor<&[u8]>) -> Result<ActionType, ActionError> {
    let raw = src.get_u8();
    let r#type = ActionType::try_from(raw).map_err(|_| ActionError::InvalidActionType(raw))?;
    Ok(r#type)
}

pub fn parse_action_scope(src: &mut Cursor<&[u8]>) -> Result<ActionVarScope, ActionError> {
    let raw = src.get_u8();
    let r#type = ActionVarScope::try_from(raw).map_err(|_| ActionError::InvalidActionScope(raw))?;
    Ok(r#type)
}

pub fn parse_list_of_messages(
    src: &mut Cursor<&[u8]>,
) -> Result<HashMap<String, HashMap<String, TypedData>>, ListOfMessagesError> {
    let mut messages = HashMap::<String, HashMap<String, TypedData>>::new();
    while src.has_remaining() {
        let message_name =
            parse_string(src).map_err(|e| ListOfMessagesError::InvalidMessageName(e))?;
        let nb_args = src.get_u8();

        let mut message_content = HashMap::<String, TypedData>::new();
        for _ in 0..nb_args {
            let name = parse_string(src).map_err(|e| ListOfMessagesError::InvalidKVListName(e))?;
            let value =
                parse_typed_data(src).map_err(|e| ListOfMessagesError::InvalidKVListValue(e))?;
            message_content.insert(name, value);
        }

        messages.insert(message_name, message_content);
    }
    Ok(messages)
}

pub fn parse_kv_list(src: &mut Cursor<&[u8]>) -> Result<HashMap<String, TypedData>, KVListError> {
    let mut body = HashMap::<String, TypedData>::new();
    while src.has_remaining() {
        let name = parse_string(src).map_err(|e| KVListError::InvalidKVListName(e))?;
        let value = parse_typed_data(src).map_err(|e| KVListError::InvalidKVListValue(e))?;
        body.insert(name, value);
    }
    Ok(body)
}

pub fn write_kv_list(dst: &mut BytesMut, hash: &HashMap<String, TypedData>) -> Result<(), Error> {
    for (k, v) in hash {
        write_string(dst, k).unwrap();
        write_typed_data(dst, v).unwrap();
    }
    Ok(())
}

pub fn parse_typed_data(src: &mut Cursor<&[u8]>) -> Result<TypedData, TypedDataError> {
    let raw = src.get_u8();
    let r#type: TypedDataType =
        TypedDataType::try_from(raw & 0x0F_u8).map_err(|_| TypedDataError::InvalidType(raw))?;
    let value = match r#type {
        TypedDataType::NULL => TypedData::NULL,
        TypedDataType::BOOL => TypedData::BOOL(raw & 0x10_u8 == 0x10_u8),
        TypedDataType::INT32 => {
            let raw = parse_varint(src)
                .map_err(|e| TypedDataError::NumberParsingError(TypedDataType::INT32, e))?;
            let value = i32::try_from(raw)
                .map_err(|_| TypedDataError::NumberConversionError(TypedDataType::INT32, raw))?;
            TypedData::INT32(value)
        }
        TypedDataType::UINT32 => {
            let raw = parse_varint(src)
                .map_err(|e| TypedDataError::NumberParsingError(TypedDataType::UINT32, e))?;
            let value = u32::try_from(raw)
                .map_err(|_| TypedDataError::NumberConversionError(TypedDataType::UINT32, raw))?;
            TypedData::UINT32(value)
        }
        TypedDataType::INT64 => {
            let raw = parse_varint(src)
                .map_err(|e| TypedDataError::NumberParsingError(TypedDataType::INT64, e))?;
            let value = raw as i64;
            TypedData::INT64(value)
        }
        TypedDataType::UINT64 => {
            let raw = parse_varint(src)
                .map_err(|e| TypedDataError::NumberParsingError(TypedDataType::UINT64, e))?;
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
            return Err(TypedDataError::NotSupported);
        }
    };

    Ok(value)
}

pub fn write_typed_data(dst: &mut BytesMut, value: &TypedData) -> Result<(), Error> {
    match value {
        TypedData::NULL => {
            dst.put_u8(0);
            Ok(())
        }
        TypedData::BOOL(v) => {
            dst.put_u8(if true == *v {
                0b_0001_0001_u8
            } else {
                0b_0000_0001_u8
            });
            Ok(())
        }
        TypedData::INT32(v) => {
            dst.put_u8(0b_0000_0010_u8);
            write_varint(dst, *v as u64)
        }
        TypedData::UINT32(v) => {
            dst.put_u8(0b_0000_0011_u8);
            write_varint(dst, *v as u64)
        }
        TypedData::INT64(v) => {
            dst.put_u8(0b_0000_0100_u8);
            write_varint(dst, *v as u64)
        }
        TypedData::UINT64(v) => {
            dst.put_u8(0b_0000_0101_u8);
            write_varint(dst, *v as u64)
        }
        TypedData::IPV4(addr) => {
            dst.put_u8(0b_0000_0110_u8);
            dst.put_slice(addr.octets().as_ref());
            Ok(())
        }
        TypedData::IPV6(addr) => {
            dst.put_u8(0b_0000_0111_u8);
            dst.put_slice(addr.octets().as_ref());
            Ok(())
        }
        TypedData::STRING(v) => {
            dst.put_u8(0b_0000_1000_u8);
            write_string(dst, v)
        }
        TypedData::BINARY(_) => {
            dst.put_u8(0b_0000_1001_u8);
            Err(Error::NotSupported)
        }
    }
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
        std::str::from_utf8(&bytes[..])
            .map_err(|e| StringError::Utf8Error(e.to_string()))
            .unwrap()
            .to_string()
    };
    Ok(val)
}

pub fn write_string(dst: &mut BytesMut, value: &String) -> Result<(), Error> {
    let bytes = value.as_bytes();
    let len = value.len();
    write_varint(dst, len as u64).unwrap();
    dst.put_slice(bytes);
    Ok(())
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

pub fn write_frame_header(dst: &mut BytesMut, frame_header: &FrameHeader) -> Result<(), Error> {
    let _ = &frame_header.r#type.write_to(dst).unwrap();
    let frame_flags = &frame_header.flags;
    let frame_flags_raw: u32 = frame_flags.0;
    dst.put_u32(frame_flags_raw);
    write_varint(dst, frame_header.stream_id).unwrap();
    write_varint(dst, frame_header.frame_id).unwrap();
    Ok(())
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

pub fn write_varint(dst: &mut BytesMut, value: u64) -> Result<(), Error> {
    if value < 240 {
        dst.put_u8(value as u8);
    } else {
        let mut value = value;

        dst.put_u8((value % 256 | 240) as u8);

        value = (value - 240) >> 4;
        while value >= 128 {
            dst.put_u8((value % 256 | 128) as u8);
            value = (value - 128) >> 7;
        }

        dst.put_u8(value as u8);
    }
    Ok(())
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
            Error::InvalidCursor {
                remaining,
                expected,
            } => write!(
                f,
                "InvalidCursor expected: {}, remaining: {}",
                expected, remaining
            ),
            Error::FragmentedModeNotSupported => write!(f, "FragmentedModeNotSupported"),
            Error::NotSupported => write!(f, "NotSupported"),
            Error::Disconnect => write!(f, "Disconnect"),
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
            FramePayloadError::InsufficientBytes => write!(f, "FramePayload::InsufficientBytes"),
            FramePayloadError::InvalidKVList(err) => {
                write!(f, "FramePayload::InvalidKVList {}", err)
            }
            FramePayloadError::NotSupported(r#type) => write!(f, "NotSupported {}", r#type),
            FramePayloadError::InvalidListOfMessages(err) => {
                write!(f, "FramePayload::InvalidListOfMessages {}", err)
            }
            FramePayloadError::InvalidListOfActions(err) => {
                write!(f, "FramePayload::InvalidListOfActions {}", err)
            }
        }
    }
}

impl fmt::Display for FrameType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            FrameType::UNSET => write!(f, "UNSET"),
            FrameType::HAPROXY_HELLO => write!(f, "HAPROXY_HELLO"),
            FrameType::HAPROXY_DISCONNECT => write!(f, "HAPROXY_DISCONNECT"),
            FrameType::NOTIFY => write!(f, "NOTIFY"),
            FrameType::AGENT_HELLO => write!(f, "AGENT_HELLO"),
            FrameType::AGENT_DISCONNECT => write!(f, "AGENT_DISCONNECT"),
            FrameType::ACK => write!(f, "ACK"),
        }
    }
}

impl fmt::Display for ActionError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ActionError::InsufficientBytes => write!(f, "ActionError::InsufficientBytes"),
            ActionError::InvalidActionType(r#type) => {
                write!(f, "ActionError::InvalidActionType {}", r#type)
            }
            ActionError::InvalidActionScope(r#type) => {
                write!(f, "ActionError::InvalidActionScope {}", r#type)
            }
            ActionError::InvalidNumberOfArgs(r#type, expected, actual) => write!(
                f,
                "ActionError::InvalidNumberOfArgs {}, got {} expected {}",
                r#type, actual, expected
            ),
            ActionError::InvalidSetVarActionVarName(err) => {
                write!(f, "ActionError::InvalidSetVarActionVarName {}", err)
            }
            ActionError::InvalidSetVarActionVarValue(err) => {
                write!(f, "ActionError::InvalidSetVarActionVarValue {}", err)
            }
            ActionError::InvalidUnsetVarActionVarName(err) => {
                write!(f, "ActionError::InvalidUnsetVarActionVarName {}", err)
            }
        }
    }
}

impl fmt::Display for ActionType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ActionType::SET_VAR => write!(f, "SET_VAR"),
            ActionType::UNSET_VAR => write!(f, "UNSET_VAR"),
        }
    }
}

impl fmt::Display for ListOfActionsError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ListOfActionsError::InvalidAction(err) => {
                write!(f, "ListOfActions::InvalidAction {}", err)
            }
        }
    }
}

impl fmt::Display for ListOfMessagesError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ListOfMessagesError::InsufficientBytes => {
                write!(f, "ListOfMessages::InsufficientBytes")
            }
            ListOfMessagesError::InvalidKVListName(err) => {
                write!(f, "ListOfMessages::InvalidKVListName {}", err)
            }
            ListOfMessagesError::InvalidKVListValue(err) => {
                write!(f, "ListOfMessages::InvalidKVListValue {}", err)
            }
            ListOfMessagesError::InvalidMessageName(err) => {
                write!(f, "ListOfMessages::InvalidMessageName {}", err)
            }
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
            TypedDataError::InvalidType(r#type) => {
                write!(f, "TypedDataError::InvalidType {}", r#type)
            }
            TypedDataError::InvalidString(err) => {
                write!(f, "TypedDataError::InvalidString {}", err)
            }
            TypedDataError::InvalidIpv4(err) => write!(f, "TypedDataError::InvalidIpv4 {}", err),
            TypedDataError::InvalidIpv6(err) => write!(f, "TypedDataError::InvalidIpv6 {}", err),
            TypedDataError::NotSupported => write!(f, "TypedDataError::NotSupported"),
            TypedDataError::NumberConversionError(data_type, r#u64) => write!(
                f,
                "TypedDataError::NumberConversionError ({}, {})",
                data_type, r#u64
            ),
            TypedDataError::NumberParsingError(data_type, err) => {
                write!(f, "TypedDataError::InvalidIpv6 ({}, {})", data_type, err)
            }
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
