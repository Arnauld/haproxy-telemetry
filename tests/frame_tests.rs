use bytes::BytesMut;
use haproxy_spoa_rust::frame::{Error, Frame, KVList, TypedData};
use std::fmt::Write;
use std::io::Cursor;

fn to_hex_string(raw: &[u8]) -> String {
    let mut s = String::new();
    for x in raw {
        if s.len() > 0 {
            write!(&mut s, ", ").unwrap();
        }
        write!(&mut s, "{:X}", x).unwrap();
    }
    s.to_lowercase()
}

fn from_hex_string(raw: &str) -> Vec<u8> {
    raw.split(", ")
        .map(|sub| u8::from_str_radix(sub, 16).ok().unwrap())
        .collect()
}

fn assert_content_contains_string(content: &KVList, key: &str, value: &str) {
    match content
        .iter()
        .find(|(k, _)| k == key)
        .expect(format!("Key not found: '{}' in {:?}", key, content).as_str())
    {
        (_, TypedData::STRING(s)) => assert_eq!(s, &value.to_string()),
        _ => panic!(
            "Invalid value type associated to key {}: {:?}",
            key, content
        ),
    };
}

fn assert_content_contains_uint32(content: &KVList, key: &str, value: u32) {
    match content
        .iter()
        .find(|(k, _)| k == key)
        .expect(format!("Key not found: '{}' in {:?}", key, content).as_str())
    {
        (_, TypedData::UINT32(v)) => assert_eq!(v, &value),
        _ => panic!(
            "Invalid value type associated to key {}: {:?}",
            key, content
        ),
    };
}

fn parse_frame(raw: &str) -> Result<Frame, Error> {
    let raw_bytes = from_hex_string(raw);
    let mut buff = Cursor::new(&raw_bytes[..]);
    Frame::parse(&mut buff)
}

fn write_frame(frame: &Frame) -> String {
    let mut full = BytesMut::new();
    Frame::write_to(frame, &mut full).unwrap();

    to_hex_string(&mut full[..])
}

#[allow(non_snake_case)]
#[test]
fn should_parse_HAProxyHello_frame() {
    let result = parse_frame("0, 0, 0, 81, 1, 0, 0, 0, 1, 0, 0, 12, 73, 75, 70, 70, 6f, 72, 74, 65, 64, 2d, 76, 65, 72, 73, 69, 6f, 6e, 73, 8, 3, 32, 2e, 30, e, 6d, 61, 78, 2d, 66, 72, 61, 6d, 65, 2d, 73, 69, 7a, 65, 3, fc, f0, 6, c, 63, 61, 70, 61, 62, 69, 6c, 69, 74, 69, 65, 73, 8, 10, 70, 69, 70, 65, 6c, 69, 6e, 69, 6e, 67, 2c, 61, 73, 79, 6e, 63, 9, 65, 6e, 67, 69, 6e, 65, 2d, 69, 64, 8, 24, 61, 33, 31, 61, 64, 30, 65, 64, 2d, 62, 62, 36, 39, 2d, 34, 36, 63, 35, 2d, 39, 66, 35, 63, 2d, 62, 32, 30, 33, 62, 62, 35, 39, 61, 38, 37, 61");
    assert!(result.is_ok());
    match result {
        Ok(Frame::HAProxyHello { header, content }) => {
            assert_eq!(header.frame_id, 0);
            assert_eq!(header.stream_id, 0);
            assert_eq!(header.flags.is_fin(), true);
            assert_eq!(header.flags.is_abort(), false);
            assert_content_contains_string(&content, "supported-versions", "2.0");
            assert_content_contains_uint32(&content, "max-frame-size", 16380_u32);
            assert_content_contains_string(&content, "capabilities", "pipelining,async");
            assert_content_contains_string(
                &content,
                "engine-id",
                "a31ad0ed-bb69-46c5-9f5c-b203bb59a87a",
            );
        }
        _ => panic!("Invalid frame parsed: {:?}", result),
    }
}

#[allow(non_snake_case)]
#[test]
fn should_parse_AgentHello_frame() {
    let result = parse_frame("0, 0, 0, 46, 65, 0, 0, 0, 1, 0, 0, 7, 76, 65, 72, 73, 69, 6f, 6e, 8, 3, 32, 2e, 30, e, 6d, 61, 78, 2d, 66, 72, 61, 6d, 65, 2d, 73, 69, 7a, 65, 3, fc, f0, 6, c, 63, 61, 70, 61, 62, 69, 6c, 69, 74, 69, 65, 73, 8, 10, 70, 69, 70, 65, 6c, 69, 6e, 69, 6e, 67, 2c, 61, 73, 79, 6e, 63");
    assert!(result.is_ok());
    match result {
        Ok(Frame::AgentHello { header, content }) => {
            assert_eq!(header.frame_id, 0);
            assert_eq!(header.stream_id, 0);
            assert_eq!(header.flags.is_fin(), true);
            assert_eq!(header.flags.is_abort(), false);
            assert_content_contains_string(&content, "version", "2.0");
            assert_content_contains_uint32(&content, "max-frame-size", 16380_u32);
            assert_content_contains_string(&content, "capabilities", "pipelining,async");
        }
        _ => panic!("Invalid frame parsed: {:?}", result),
    }
}

#[allow(non_snake_case)]
#[test]
fn should_parse_Notify_frame() {
    let result = parse_frame("0, 0, 0, 8b, 3, 0, 0, 0, 1, 2, 2, 20, 6f, 70, 65, 6e, 74, 72, 61, 63, 69, 6e, 67, 3a, 66, 72, 6f, 6e, 74, 65, 6e, 64, 5f, 74, 63, 70, 5f, 72, 65, 71, 75, 65, 73, 74, 3, 2, 69, 64, 8, 29, 36, 31, 62, 35, 37, 65, 66, 30, 2d, 32, 34, 62, 62, 2d, 34, 32, 63, 37, 2d, 38, 39, 33, 35, 2d, 61, 65, 64, 64, 32, 37, 36, 61, 66, 34, 61, 35, 3a, 30, 30, 30, 38, 4, 73, 70, 61, 6e, 8, 14, 46, 72, 6f, 6e, 74, 65, 6e, 64, 20, 54, 43, 50, 20, 72, 65, 71, 75, 65, 73, 74, 8, 63, 68, 69, 6c, 64, 2d, 6f, 66, 8, e, 43, 6c, 69, 65, 6e, 74, 20, 73, 65, 73, 73, 69, 6f, 6e");
    assert!(result.is_ok());
    match result {
        Ok(Frame::Notify { header, messages }) => {
            assert_eq!(header.frame_id, 2);
            assert_eq!(header.stream_id, 2);
            assert_eq!(header.flags.is_fin(), true);
            assert_eq!(header.flags.is_abort(), false);
            let msg = messages
                .get("opentracing:frontend_tcp_request")
                .expect("<opentracing:frontend_tcp_request> message not found");
            assert_content_contains_string(&msg, "id", "61b57ef0-24bb-42c7-8935-aedd276af4a5:0008");
            assert_content_contains_string(&msg, "span", "Frontend TCP request");
            assert_content_contains_string(&msg, "child-of", "Client session");
        }
        _ => panic!("Invalid frame parsed: {:?}", result),
    }
}

#[allow(non_snake_case)]
#[test]
fn decode_encode_should_lead_to_the_same_result__AgentHello_frame() {
    let raw = "0, 0, 0, 46, 65, 0, 0, 0, 1, 0, 0, 7, 76, 65, 72, 73, 69, 6f, 6e, 8, 3, 32, 2e, 30, e, 6d, 61, 78, 2d, 66, 72, 61, 6d, 65, 2d, 73, 69, 7a, 65, 3, fc, f0, 6, c, 63, 61, 70, 61, 62, 69, 6c, 69, 74, 69, 65, 73, 8, 10, 70, 69, 70, 65, 6c, 69, 6e, 69, 6e, 67, 2c, 61, 73, 79, 6e, 63";
    let frame = parse_frame(raw).unwrap();
    let encoded = write_frame(&frame);
    assert_eq!(raw, encoded);
}

#[allow(non_snake_case)]
#[test]
fn decode_encode_should_lead_to_the_same_result__Ack_frame() {
    let raw = "0, 0, 0, 7, 67, 0, 0, 0, 1, 2, 1";
    let frame = parse_frame(raw).unwrap();
    let encoded = write_frame(&frame);
    assert_eq!(raw, encoded);
}
