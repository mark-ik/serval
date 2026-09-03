// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

use fleece::{TextFragment, TextQuoteSelector, text_fragment};

fn quote(exact: &str, prefix: &str, suffix: &str) -> TextQuoteSelector {
    TextQuoteSelector {
        exact: exact.into(),
        prefix: prefix.into(),
        suffix: suffix.into(),
    }
}

fn decode_term(encoded: &str) -> String {
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap();
            decoded.push(u8::from_str_radix(hex, 16).unwrap());
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).unwrap()
}

fn resolve_directive(document: &str, directive: &str) -> Vec<(u64, u64)> {
    let value = directive.strip_prefix(":~:text=").unwrap();
    let (without_suffix, suffix) = value
        .rsplit_once(",-")
        .map_or((value, ""), |(head, tail)| (head, tail));
    let (prefix, exact) = without_suffix
        .split_once("-,")
        .map_or(("", without_suffix), |(head, tail)| (head, tail));
    let prefix = decode_term(prefix);
    let exact = decode_term(exact);
    let suffix = decode_term(suffix);

    document
        .match_indices(&exact)
        .filter(|(byte_start, _)| {
            document[..*byte_start].ends_with(&prefix)
                && document[*byte_start + exact.len()..].starts_with(&suffix)
        })
        .map(|(byte_start, _)| {
            let start = document[..byte_start].chars().count() as u64;
            (start, start + exact.chars().count() as u64)
        })
        .collect()
}

#[test]
fn projects_all_text_directive_context_terms() {
    let selector = quote("start, end & 50%-", "before -", " - after");
    let result = text_fragment(&selector).expect("non-empty exact");
    assert_eq!(
        result,
        TextFragment {
            directive: ":~:text=before%20%2D-,start%2C%20end%20%26%2050%25%2D,-%20%2D%20after"
                .into()
        }
    );
}

#[test]
fn encodes_non_ascii_and_bidi_as_utf8() {
    let selector = quote("café שלום", "前", "後");
    assert_eq!(
        text_fragment(&selector).unwrap().directive,
        ":~:text=%E5%89%8D-,caf%C3%A9%20%D7%A9%D7%9C%D7%95%D7%9D,-%E5%BE%8C"
    );
}

#[test]
fn omits_absent_context_and_rejects_empty_exact() {
    assert_eq!(
        text_fragment(&quote("hello", "", "")).unwrap().directive,
        ":~:text=hello"
    );
    assert_eq!(text_fragment(&quote("", "before", "after")), None);
}

#[test]
fn repeated_quote_directive_resolves_to_the_selector_range() {
    let document = "Intro. Repeat this. Middle. Repeat this. End.";
    let selector = quote("Repeat this.", "Middle. ", " End.");
    let second_byte = document.match_indices(&selector.exact).nth(1).unwrap().0;
    let start = document[..second_byte].chars().count() as u64;
    let expected = (start, start + selector.exact.chars().count() as u64);
    let directive = text_fragment(&selector).unwrap();
    assert_eq!(
        resolve_directive(document, &directive.directive),
        vec![expected]
    );
}
