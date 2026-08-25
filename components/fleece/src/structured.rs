/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use layout_dom_api::LayoutDom;

use crate::{attr, collect_text, local_name};

/// One JSON value harvested from page-carried structured data.
///
/// Fleece keeps this small value model locally so JSON-LD does not add a
/// parser dependency to the render-free extraction cone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StructuredValue {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<StructuredValue>),
    Object(Vec<(String, StructuredValue)>),
}

impl StructuredValue {
    /// Return an object member by name.
    pub fn get(&self, name: &str) -> Option<&StructuredValue> {
        let Self::Object(entries) = self else {
            return None;
        };
        entries
            .iter()
            .find_map(|(key, value)| (key == name).then_some(value))
    }

    /// Return this value as a string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }
}

/// A typed block harvested from JSON-LD or HTML microdata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuredData {
    /// The declared schema type (`Recipe`, `Event`, `Person`, `Article`, ...).
    pub kind: String,
    /// The harvested value, retaining fields fleece does not interpret.
    pub value: StructuredValue,
    /// The page syntax that supplied the value.
    pub source: StructuredDataSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuredDataSource {
    JsonLd,
    Microdata,
}

/// Harvest JSON-LD and microdata without interpreting consumer policy.
pub fn extract_structured_data<D: LayoutDom>(dom: &D) -> Vec<StructuredData> {
    let mut out = Vec::new();
    walk_structured_data(dom, dom.document(), &mut out);
    out
}

fn walk_structured_data<D: LayoutDom>(dom: &D, id: D::NodeId, out: &mut Vec<StructuredData>) {
    if local_name(dom, id) == Some("script")
        && attr(dom, id, "type").is_some_and(|value| {
            value
                .split(';')
                .next()
                .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/ld+json"))
        })
    {
        let raw = raw_text_of(dom, id);
        if let Some(value) = JsonParser::new(&raw).parse() {
            collect_json_ld(value, out);
        }
    }
    if attr(dom, id, "itemscope").is_some()
        && let Some(data) = microdata_item(dom, id)
    {
        out.push(data);
    }
    for child in dom.dom_children(id) {
        walk_structured_data(dom, child, out);
    }
}

fn collect_json_ld(value: StructuredValue, out: &mut Vec<StructuredData>) {
    match value {
        StructuredValue::Array(values) => {
            for value in values {
                collect_json_ld(value, out);
            }
        },
        StructuredValue::Object(entries) => {
            let value = StructuredValue::Object(entries);
            if let Some(kind) = json_ld_kind(&value) {
                out.push(StructuredData {
                    kind,
                    value: value.clone(),
                    source: StructuredDataSource::JsonLd,
                });
            }
            if let Some(StructuredValue::Array(graph)) = value.get("@graph") {
                for child in graph.clone() {
                    collect_json_ld(child, out);
                }
            }
        },
        _ => {},
    }
}

fn json_ld_kind(value: &StructuredValue) -> Option<String> {
    match value.get("@type")? {
        StructuredValue::String(kind) => Some(short_schema_type(kind)),
        StructuredValue::Array(kinds) => kinds
            .iter()
            .find_map(StructuredValue::as_str)
            .map(short_schema_type),
        _ => None,
    }
}

fn short_schema_type(value: &str) -> String {
    value.rsplit(['/', '#']).next().unwrap_or(value).to_string()
}

fn microdata_item<D: LayoutDom>(dom: &D, root: D::NodeId) -> Option<StructuredData> {
    let kind = attr(dom, root, "itemtype")
        .and_then(|types| types.split_whitespace().next().map(short_schema_type))?;
    let mut fields = Vec::new();
    collect_microdata_fields(dom, root, root, &mut fields);
    Some(StructuredData {
        kind,
        value: StructuredValue::Object(fields),
        source: StructuredDataSource::Microdata,
    })
}

fn collect_microdata_fields<D: LayoutDom>(
    dom: &D,
    root: D::NodeId,
    id: D::NodeId,
    fields: &mut Vec<(String, StructuredValue)>,
) {
    if id != root && attr(dom, id, "itemscope").is_some() {
        if let Some(property) = attr(dom, id, "itemprop")
            && let Some(item) = microdata_item(dom, id)
        {
            fields.push((property, item.value));
        }
        return;
    }
    if id != root
        && let Some(property) = attr(dom, id, "itemprop")
    {
        let value = attr(dom, id, "content")
            .or_else(|| attr(dom, id, "datetime"))
            .or_else(|| attr(dom, id, "href"))
            .or_else(|| attr(dom, id, "src"))
            .unwrap_or_else(|| crate::text_of(dom, id));
        if !value.is_empty() {
            for property in property.split_whitespace() {
                fields.push((property.to_string(), StructuredValue::String(value.clone())));
            }
        }
    }
    for child in dom.dom_children(id) {
        collect_microdata_fields(dom, root, child, fields);
    }
}

fn raw_text_of<D: LayoutDom>(dom: &D, id: D::NodeId) -> String {
    let mut out = String::new();
    collect_text(dom, id, &mut out);
    out
}

struct JsonParser<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl<'a> JsonParser<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            bytes: source.as_bytes(),
            cursor: 0,
        }
    }

    fn parse(mut self) -> Option<StructuredValue> {
        self.skip_ws();
        let value = self.value()?;
        self.skip_ws();
        (self.cursor == self.bytes.len()).then_some(value)
    }

    fn value(&mut self) -> Option<StructuredValue> {
        self.skip_ws();
        match self.peek()? {
            b'n' => self.literal(b"null", StructuredValue::Null),
            b't' => self.literal(b"true", StructuredValue::Bool(true)),
            b'f' => self.literal(b"false", StructuredValue::Bool(false)),
            b'"' => self.string().map(StructuredValue::String),
            b'[' => self.array(),
            b'{' => self.object(),
            b'-' | b'0'..=b'9' => self.number().map(StructuredValue::Number),
            _ => None,
        }
    }

    fn literal(&mut self, literal: &[u8], value: StructuredValue) -> Option<StructuredValue> {
        let end = self.cursor.checked_add(literal.len())?;
        (self.bytes.get(self.cursor..end)? == literal).then(|| {
            self.cursor = end;
            value
        })
    }

    fn array(&mut self) -> Option<StructuredValue> {
        self.take(b'[')?;
        let mut values = Vec::new();
        self.skip_ws();
        if self.take(b']').is_some() {
            return Some(StructuredValue::Array(values));
        }
        loop {
            values.push(self.value()?);
            self.skip_ws();
            if self.take(b']').is_some() {
                break;
            }
            self.take(b',')?;
        }
        Some(StructuredValue::Array(values))
    }

    fn object(&mut self) -> Option<StructuredValue> {
        self.take(b'{')?;
        let mut entries = Vec::new();
        self.skip_ws();
        if self.take(b'}').is_some() {
            return Some(StructuredValue::Object(entries));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            self.take(b':')?;
            entries.push((key, self.value()?));
            self.skip_ws();
            if self.take(b'}').is_some() {
                break;
            }
            self.take(b',')?;
        }
        Some(StructuredValue::Object(entries))
    }

    fn string(&mut self) -> Option<String> {
        self.take(b'"')?;
        let mut out = String::new();
        while let Some(byte) = self.peek() {
            self.cursor += 1;
            match byte {
                b'"' => return Some(out),
                b'\\' => {
                    let escaped = self.peek()?;
                    self.cursor += 1;
                    match escaped {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => out.push(self.unicode_escape()?),
                        _ => return None,
                    }
                },
                0x00..=0x1f => return None,
                ascii if ascii.is_ascii() => out.push(ascii as char),
                _ => {
                    self.cursor -= 1;
                    let tail = std::str::from_utf8(&self.bytes[self.cursor..]).ok()?;
                    let character = tail.chars().next()?;
                    self.cursor += character.len_utf8();
                    out.push(character);
                },
            }
        }
        None
    }

    fn unicode_escape(&mut self) -> Option<char> {
        let first = self.hex_quad()?;
        if (0xd800..=0xdbff).contains(&first) {
            self.take(b'\\')?;
            self.take(b'u')?;
            let second = self.hex_quad()?;
            if !(0xdc00..=0xdfff).contains(&second) {
                return None;
            }
            let scalar =
                0x1_0000 + ((u32::from(first) - 0xd800) << 10) + (u32::from(second) - 0xdc00);
            char::from_u32(scalar)
        } else {
            char::from_u32(u32::from(first))
        }
    }

    fn hex_quad(&mut self) -> Option<u16> {
        let mut value = 0u16;
        for _ in 0..4 {
            let digit = (self.peek()? as char).to_digit(16)? as u16;
            self.cursor += 1;
            value = value.checked_mul(16)?.checked_add(digit)?;
        }
        Some(value)
    }

    fn number(&mut self) -> Option<String> {
        let start = self.cursor;
        if self.peek() == Some(b'-') {
            self.cursor += 1;
        }
        match self.peek()? {
            b'0' => self.cursor += 1,
            b'1'..=b'9' => {
                self.cursor += 1;
                while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                    self.cursor += 1;
                }
            },
            _ => return None,
        }
        if self.peek() == Some(b'.') {
            self.cursor += 1;
            let fraction = self.cursor;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.cursor += 1;
            }
            if self.cursor == fraction {
                return None;
            }
        }
        if self.peek().is_some_and(|byte| matches!(byte, b'e' | b'E')) {
            self.cursor += 1;
            if self.peek().is_some_and(|byte| matches!(byte, b'+' | b'-')) {
                self.cursor += 1;
            }
            let exponent = self.cursor;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.cursor += 1;
            }
            if self.cursor == exponent {
                return None;
            }
        }
        std::str::from_utf8(&self.bytes[start..self.cursor])
            .ok()
            .map(str::to_string)
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
            self.cursor += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.cursor).copied()
    }

    fn take(&mut self, expected: u8) -> Option<()> {
        (self.peek()? == expected).then(|| self.cursor += 1)
    }
}
