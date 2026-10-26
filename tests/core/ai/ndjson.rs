//! NDJSON 行切分（Ollama native）

use crate::core::ai::ndjson::NdjsonParser;

#[test]
fn 一次一整行() {
    let mut p = NdjsonParser::new();
    let mut out = Vec::new();
    p.push("{\"a\":1}\n", &mut out);
    assert_eq!(out, vec!["{\"a\":1}".to_string()]);
}

#[test]
fn 半行跨_read() {
    let mut p = NdjsonParser::new();
    let mut out = Vec::new();
    p.push("{\"a\":", &mut out);
    assert!(out.is_empty());
    p.push("1}\n{\"b\":2}\n", &mut out);
    assert_eq!(out, vec!["{\"a\":1}".to_string(), "{\"b\":2}".to_string()]);
}

#[test]
fn crlf_与空行() {
    let mut p = NdjsonParser::new();
    let mut out = Vec::new();
    p.push("\r\n{\"a\":1}\r\n\r\n", &mut out);
    assert_eq!(out, vec!["{\"a\":1}".to_string()]);
}

#[test]
fn finish_吐出残留() {
    let mut p = NdjsonParser::new();
    let mut out = Vec::new();
    p.push("{\"a\":1}", &mut out);
    assert!(out.is_empty());
    p.finish(&mut out);
    assert_eq!(out, vec!["{\"a\":1}".to_string()]);
}
