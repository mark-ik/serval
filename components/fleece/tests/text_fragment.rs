#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextQuoteSelector {
    pub exact: String,
    pub prefix: String,
    pub suffix: String,
}

#[path = "../src/text_fragment.rs"]
mod text_fragment;

use text_fragment::{text_fragment, TextFragment};

fn quote(exact: &str, prefix: &str, suffix: &str) -> TextQuoteSelector {
    TextQuoteSelector {
        exact: exact.into(),
        prefix: prefix.into(),
        suffix: suffix.into(),
    }
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
