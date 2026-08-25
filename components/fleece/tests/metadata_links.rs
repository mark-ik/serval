use fleece::extract_metadata;
use genet_static_dom::StaticDocument;

fn fixture() -> &'static str {
    include_str!("fixtures/metadata/links.html")
}

#[test]
fn groups_open_graph_without_losing_order_or_unknowns() {
    let metadata = extract_metadata(&StaticDocument::parse(fixture()));
    assert_eq!(
        metadata.open_graph,
        vec![
            ("ghost:detail".into(), "orphan before root".into()),
            ("image".into(), "/image.png".into()),
            ("image:width".into(), "1200".into()),
            ("image:height".into(), "630".into()),
            ("unknown".into(), "kept".into()),
            ("unknown:detail".into(), "also kept".into()),
            ("title".into(), "A title".into()),
            ("image:orphan".into(), "after different root".into()),
            ("image".into(), "/second.png".into()),
            ("image:alt".into(), "second image".into()),
        ]
    );
    assert_eq!(metadata.open_graph_groups.len(), 4);
    assert_eq!(metadata.open_graph_groups[0].property, "image");
    assert_eq!(metadata.open_graph_groups[0].value, "/image.png");
    assert_eq!(
        metadata.open_graph_groups[0].structured,
        vec![
            ("width".into(), "1200".into()),
            ("height".into(), "630".into())
        ]
    );
    assert_eq!(metadata.open_graph_groups[1].property, "unknown");
    assert_eq!(metadata.open_graph_groups[1].value, "kept");
    assert_eq!(
        metadata.open_graph_groups[1].structured,
        vec![("detail".into(), "also kept".into())]
    );
    assert_eq!(metadata.open_graph_groups[2].property, "title");
    assert_eq!(metadata.open_graph_groups[2].value, "A title");
    assert!(metadata.open_graph_groups[2].structured.is_empty());
    assert_eq!(metadata.open_graph_groups[3].property, "image");
    assert_eq!(metadata.open_graph_groups[3].value, "/second.png");
    assert_eq!(
        metadata.open_graph_groups[3].structured,
        vec![("alt".into(), "second image".into())]
    );
}

#[test]
fn extracts_html_links_with_raw_values_and_relation_rules() {
    let metadata = extract_metadata(&StaticDocument::parse(fixture()));
    assert_eq!(metadata.canonical.as_deref(), Some("../canonical"));
    assert_eq!(metadata.links.len(), 4);

    let canonical = &metadata.links[0];
    assert_eq!(canonical.rel, vec!["canonical", "alternate", "preload"]);
    assert!(canonical.has_relation("CANONICAL"));
    assert_eq!(canonical.href.as_deref(), Some("../canonical"));
    assert_eq!(canonical.type_.as_deref(), Some("text/html"));
    assert_eq!(canonical.hreflang.as_deref(), Some("en-US"));
    assert_eq!(canonical.title.as_deref(), Some("Canonical"));
    assert_eq!(canonical.media.as_deref(), Some("screen"));
    assert_eq!(
        canonical.other,
        vec![
            ("data-owner".into(), "test".into()),
            ("id".into(), "main-link".into())
        ]
    );

    let extension = &metadata.links[1];
    assert_eq!(extension.rel, vec!["https://EXAMPLE.test/Rel", "next"]);
    assert!(extension.has_relation("https://example.TEST/rel"));
    assert_eq!(extension.href.as_deref(), Some("//cdn.example/next"));
    assert_eq!(extension.type_, None);
    assert_eq!(extension.hreflang, None);
    assert_eq!(extension.title, None);
    assert_eq!(extension.media, None);
    assert_eq!(extension.other, Vec::<(String, String)>::new());

    let no_href = &metadata.links[2];
    assert_eq!(no_href.rel, vec!["stylesheet"]);
    assert_eq!(no_href.href, None);
    assert_eq!(metadata.canonical.as_deref(), Some("../canonical"));

    let exact_spaces = &metadata.links[3];
    assert_eq!(
        exact_spaces.rel,
        vec!["canonical\u{000B}alternate\u{2003}preload"]
    );
    assert!(!exact_spaces.has_relation("canonical"));
}
