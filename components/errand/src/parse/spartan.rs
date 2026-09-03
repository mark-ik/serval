// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Spartan's gemtext-compatible document grammar plus its `=:` prompt line.

use super::gemtext::{GemLine, parse as parse_gemtext};

/// One line-level Spartan document construct.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpartanLine {
    /// A construct inherited unchanged from gemtext.
    Gemtext(GemLine),
    /// An input prompt. The target is resolved by the consuming document host.
    Prompt { target: String, label: String },
}

/// Parse a Spartan document without flattening its mutation affordances into
/// ordinary text or navigation links.
pub fn parse(input: &str) -> Vec<SpartanLine> {
    parse_gemtext(input)
        .into_iter()
        .map(|line| match &line {
            GemLine::Text(text) => match spartan_protocol::parse_prompt_line(text) {
                Some((target, label)) => SpartanLine::Prompt {
                    target: target.to_string(),
                    label: label.unwrap_or(target).to_string(),
                },
                None => SpartanLine::Gemtext(line),
            },
            _ => SpartanLine::Gemtext(line),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_is_typed() {
        assert_eq!(
            parse("=: /guestbook Sign it\n"),
            vec![SpartanLine::Prompt {
                target: "/guestbook".into(),
                label: "Sign it".into(),
            }]
        );
    }

    #[test]
    fn prompt_marker_inside_preformatted_text_stays_text() {
        let lines = parse("```\n=: /not-a-prompt\n```\n");
        assert!(matches!(
            lines.as_slice(),
            [SpartanLine::Gemtext(GemLine::Pre { text, .. })]
                if text.contains("=: /not-a-prompt")
        ));
    }
}
