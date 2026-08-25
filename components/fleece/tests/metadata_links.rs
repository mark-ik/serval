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
            ("image".into(), "/image.png".into()),
            ("image:width".into(), "1200".into()),
            ("image:height".into(), "630".into()),
            ("unknown".into(), "kept".into()),
            ("unknown:detail".into(), "also kept".into()),
            ("title".into(), "A title".into()),
        ]
    );
    assert_eq!(metadata.open_graph_groups.len(), 3);
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
}

#[test]
fn extracts_html_links_with_raw_values_and_relation_rules() {
    let metadata = extract_metadata(&StaticDocument::parse(fixture()));
    assert_eq!(metadata.canonical.as_deref(), Some("../canonical"));
    assert_eq!(metadata.links.len(), 3);

    let canonical = &metadata.links[0];
    assert_eq!(canonical.rel, vec!["canonical", "alternate", "preload"]);
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
}
