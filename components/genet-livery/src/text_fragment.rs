//! URL Fragment Text Directive parsing for retained-document navigation.
//!
//! This is deliberately a host navigation primitive, not an extractor feature.
//! It keeps the `:~:` directive separate from the script-visible URL while the
//! retained document later resolves a parsed directive against shaped text.

/// One successfully parsed `text=` URL Fragment Text Directive.
///
/// The terms have already been percent-decoded as UTF-8. `start` is required;
/// the context terms and range end are optional.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextDirective {
    pub prefix: Option<String>,
    pub start: String,
    pub end: Option<String>,
    pub suffix: Option<String>,
}

/// The navigation-time split between an ordinary element fragment and an
/// opaque-to-script fragment directive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NavigationFragment {
    /// URL passed to document/script state after the `:~:` suffix is removed.
    pub script_visible_url: String,
    /// Resource address without any `#` fragment, suitable for fetching.
    pub resource_url: String,
    /// The ordinary element fragment, if one preceded `:~:`.
    pub element_fragment: Option<String>,
    /// Valid text directives in source order. Unknown and malformed directives
    /// are intentionally ignored.
    pub text_directives: Vec<TextDirective>,
}

impl NavigationFragment {
    /// Separate an initial navigation URL into fetch, script, and retained
    /// directive state. The first `:~:` in the fragment is the delimiter, as
    /// required by the WICG extraction algorithm.
    pub fn parse(url: &str) -> Self {
        let Some((resource_url, raw_fragment)) = url.split_once('#') else {
            return Self {
                script_visible_url: url.to_owned(),
                resource_url: url.to_owned(),
                element_fragment: None,
                text_directives: Vec::new(),
            };
        };
        let Some((element, directive)) = raw_fragment.split_once(":~:") else {
            return Self {
                script_visible_url: url.to_owned(),
                resource_url: resource_url.to_owned(),
                element_fragment: (!raw_fragment.is_empty()).then(|| raw_fragment.to_owned()),
                text_directives: Vec::new(),
            };
        };
        Self {
            // An empty ordinary fragment remains an explicitly present `#`, the
            // same URL shape produced by setting a URL fragment to the empty
            // string in the Text Fragment specification.
            script_visible_url: format!("{resource_url}#{element}"),
            resource_url: resource_url.to_owned(),
            element_fragment: (!element.is_empty()).then(|| element.to_owned()),
            text_directives: parse_fragment_directive(directive),
        }
    }
}

/// Parse the `:~:` suffix into valid `text=` directives in source order.
pub fn parse_fragment_directive(fragment_directive: &str) -> Vec<TextDirective> {
    fragment_directive
        .split('&')
        .filter_map(|directive| directive.strip_prefix("text="))
        .filter_map(parse_text_directive)
        .collect()
}

/// Parse the value after `text=` according to the WICG Text Directive grammar.
pub fn parse_text_directive(value: &str) -> Option<TextDirective> {
    let mut terms = value.split(',').collect::<Vec<_>>();
    if terms.is_empty() || terms.len() > 4 {
        return None;
    }

    let prefix = if terms.first()?.ends_with('-') {
        let term = terms.remove(0);
        let prefix = term.strip_suffix('-')?;
        Some(decode_text_term(prefix)?)
    } else {
        None
    };
    let suffix = if terms.last()?.starts_with('-') {
        let term = terms.pop()?;
        let suffix = term.strip_prefix('-')?;
        Some(decode_text_term(suffix)?)
    } else {
        None
    };
    if terms.is_empty() || terms.len() > 2 {
        return None;
    }
    let start = decode_text_term(terms[0])?;
    let end = if terms.len() == 2 {
        Some(decode_text_term(terms[1])?)
    } else {
        None
    };

    Some(TextDirective {
        prefix,
        start,
        end,
        suffix,
    })
}

fn decode_text_term(term: &str) -> Option<String> {
    if term.is_empty() || term.contains('-') || term.contains(',') || term.contains('&') {
        return None;
    }
    let mut bytes = Vec::with_capacity(term.len());
    let source = term.as_bytes();
    let mut index = 0;
    while index < source.len() {
        match source[index] {
            b'%' => {
                let high = *source.get(index + 1)?;
                let low = *source.get(index + 2)?;
                bytes.push((hex_value(high)? << 4) | hex_value(low)?);
                index += 3;
            },
            byte if is_text_directive_explicit_byte(byte) => {
                bytes.push(byte);
                index += 1;
            },
            _ => return None,
        }
    }
    String::from_utf8(bytes).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn is_text_directive_explicit_byte(byte: u8) -> bool {
    matches!(byte,
        b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' |
        b'!' | b'$' | b'\'' | b'(' | b')' | b'*' | b'+' | b'.' | b'/' |
        b':' | b';' | b'=' | b'?' | b'@' | b'_' | b'~'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_directive_shapes_and_percent_decodes_utf8() {
        assert_eq!(
            parse_text_directive("start"),
            Some(TextDirective {
                prefix: None,
                start: "start".to_owned(),
                end: None,
                suffix: None,
            })
        );
        assert_eq!(
            parse_text_directive("start,end"),
            Some(TextDirective {
                prefix: None,
                start: "start".to_owned(),
                end: Some("end".to_owned()),
                suffix: None,
            })
        );
        assert_eq!(
            parse_text_directive("before-,start,-after"),
            Some(TextDirective {
                prefix: Some("before".to_owned()),
                start: "start".to_owned(),
                end: None,
                suffix: Some("after".to_owned()),
            })
        );
        assert_eq!(
            parse_text_directive("before-,start,end,-after"),
            Some(TextDirective {
                prefix: Some("before".to_owned()),
                start: "start".to_owned(),
                end: Some("end".to_owned()),
                suffix: Some("after".to_owned()),
            })
        );
        assert_eq!(
            parse_text_directive("caf%C3%A9"),
            Some(TextDirective {
                prefix: None,
                start: "café".to_owned(),
                end: None,
                suffix: None,
            })
        );
    }

    #[test]
    fn rejects_malformed_text_directives() {
        for value in [
            "",
            "-prefix",
            "start,",
            "prefix--,start",
            "start,-",
            "a,b,c",
        ] {
            assert!(parse_text_directive(value).is_none(), "{value}");
        }
        assert!(parse_text_directive("bad%ZZ").is_none());
        assert!(parse_text_directive("raw text").is_none());
    }

    #[test]
    fn preserves_element_fragment_and_hides_directive_from_scripts() {
        let navigation = NavigationFragment::parse(
            "https://example.test/article#section:~:text=first&unknown&text=second",
        );
        assert_eq!(navigation.resource_url, "https://example.test/article");
        assert_eq!(
            navigation.script_visible_url,
            "https://example.test/article#section"
        );
        assert_eq!(navigation.element_fragment.as_deref(), Some("section"));
        assert_eq!(
            navigation
                .text_directives
                .iter()
                .map(|directive| directive.start.as_str())
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
    }
}
