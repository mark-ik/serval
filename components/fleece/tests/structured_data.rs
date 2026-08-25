use fleece::{StructuredData, StructuredDataSource, StructuredValue, extract_structured_data};
use genet_static_dom::StaticDocument;

fn fixture(name: &str) -> &'static str {
    match name {
        "jsonld.html" => include_str!("fixtures/structured/jsonld.html"),
        "microdata.html" => include_str!("fixtures/structured/microdata.html"),
        _ => panic!("unknown fixture {name}"),
    }
}

fn object(value: &StructuredValue) -> &[(String, StructuredValue)] {
    match value {
        StructuredValue::Object(entries) => entries,
        other => panic!("expected object, got {other:?}"),
    }
}

fn field<'a>(value: &'a StructuredValue, name: &str) -> &'a StructuredValue {
    value
        .get(name)
        .unwrap_or_else(|| panic!("missing field {name}"))
}

fn string(value: &StructuredValue) -> &str {
    value
        .as_str()
        .unwrap_or_else(|| panic!("expected string, got {value:?}"))
}

fn assert_identity(
    data: &StructuredData,
    source: StructuredDataSource,
    types: &[&str],
    id: Option<&str>,
) {
    assert_eq!(data.source, source);
    assert_eq!(
        data.types,
        types
            .iter()
            .map(|value| (*value).to_owned())
            .collect::<Vec<_>>()
    );
    assert_eq!(data.id.as_deref(), id);
}

#[test]
fn json_ld_preserves_full_identity_context_values_and_order() {
    let data = extract_structured_data(&StaticDocument::parse(fixture("jsonld.html")));
    assert_eq!(data.len(), 7);

    assert_identity(
        &data[0],
        StructuredDataSource::JsonLd,
        &["https://schema.org/Person", "urn:custom:Staff"],
        Some("urn:person:1"),
    );
    assert_eq!(
        field(&data[0].value, "@context"),
        &StructuredValue::Array(vec![
            StructuredValue::String("https://schema.org/".into()),
            StructuredValue::Object(vec![(
                "ex".into(),
                StructuredValue::String("https://example.test/".into()),
            )]),
        ])
    );
    assert_eq!(string(field(&data[0].value, "name")), "Ada");
    assert_eq!(field(&data[0].value, "null"), &StructuredValue::Null);
    assert_eq!(field(&data[0].value, "flag"), &StructuredValue::Bool(true));
    assert_eq!(
        field(&data[0].value, "nested"),
        &StructuredValue::Object(vec![("n".into(), StructuredValue::Number("2".into()))])
    );

    assert_identity(
        &data[1],
        StructuredDataSource::JsonLd,
        &["urn:custom:Second"],
        Some("urn:array:2"),
    );
    assert_eq!(
        field(&data[1].value, "order"),
        &StructuredValue::Number("2".into())
    );
    assert_identity(&data[2], StructuredDataSource::JsonLd, &[], None);
    assert!(data[2].value.get("@graph").is_some());
    assert_identity(
        &data[3],
        StructuredDataSource::JsonLd,
        &[],
        Some("#untyped"),
    );
    assert_eq!(string(field(&data[3].value, "label")), "graph wrapper");
    assert_identity(
        &data[4],
        StructuredDataSource::JsonLd,
        &["https://example.test/Third", "https://example.test/Fourth"],
        Some("#third"),
    );
    assert_identity(
        &data[5],
        StructuredDataSource::JsonLd,
        &["https://example.test/Fifth"],
        Some("#fourth"),
    );
    assert_identity(
        &data[6],
        StructuredDataSource::JsonLd,
        &["urn:custom:Recovered"],
        Some("urn:recovered"),
    );
    assert_eq!(field(&data[6].value, "ok"), &StructuredValue::Bool(true));
}

#[test]
fn microdata_preserves_roots_tokens_properties_nested_items_cycles_and_itemref() {
    let data = extract_structured_data(&StaticDocument::parse(fixture("microdata.html")));
    let roots = data
        .iter()
        .filter(|item| item.source == StructuredDataSource::Microdata)
        .collect::<Vec<_>>();
    assert_eq!(
        roots.len(),
        4,
        "root, independent, values, and untyped roots"
    );

    let root = roots[0];
    assert_identity(
        root,
        StructuredDataSource::Microdata,
        &["https://schema.org/Thing", "urn:custom:Root"],
        Some("/items/root"),
    );
    let entries = object(&root.value);
    assert_eq!(
        entries
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        vec![
            "name",
            "alias",
            "tag",
            "tag",
            "author",
            "overlap",
            "generic",
            "missing-meta",
            "missing-image",
            "missing-link",
            "outside-a",
            "outside-b",
        ]
    );
    assert_eq!(
        entries.iter().filter(|(name, _)| name == "alias").count(),
        1
    );
    assert_eq!(string(field(&root.value, "tag")), "first");
    assert_eq!(
        entries
            .iter()
            .filter(|(name, _)| name == "tag")
            .map(|(_, value)| string(value))
            .collect::<Vec<_>>(),
        vec!["first", "second"]
    );
    assert_eq!(string(field(&root.value, "outside-a")), "A");
    assert_eq!(string(field(&root.value, "outside-b")), "B");
    assert_eq!(string(field(&root.value, "overlap")), "tree order overlap");
    assert_eq!(string(field(&root.value, "generic")), "A  B\n C");
    assert_eq!(string(field(&root.value, "missing-meta")), "");
    assert_eq!(string(field(&root.value, "missing-image")), "");
    assert_eq!(string(field(&root.value, "missing-link")), "");

    let author = field(&root.value, "author");
    let StructuredValue::Item(author) = author else {
        panic!("nested item must remain an Item: {author:?}");
    };
    assert_identity(
        author,
        StructuredDataSource::Microdata,
        &["https://schema.org/Person", "urn:custom:Writer"],
        Some("/people/ada"),
    );
    assert_eq!(string(field(&author.value, "name")), "Ada");
    assert!(matches!(
        field(&author.value, "related"),
        StructuredValue::Item(_)
    ));
    let related = field(&author.value, "related");
    let StructuredValue::Item(related) = related else {
        unreachable!()
    };
    assert_identity(
        related,
        StructuredDataSource::Microdata,
        &["urn:custom:Cycle"],
        Some("#cycle"),
    );
    assert_eq!(field(&related.value, "related"), &StructuredValue::Cycle);

    assert_identity(
        roots[1],
        StructuredDataSource::Microdata,
        &["urn:custom:Independent"],
        None,
    );
    assert_eq!(string(field(&roots[1].value, "name")), "Independent");
    assert_identity(
        roots[2],
        StructuredDataSource::Microdata,
        &["urn:custom:Values"],
        Some("../values"),
    );
    assert_identity(roots[3], StructuredDataSource::Microdata, &[], None);
    assert_eq!(string(field(&roots[3].value, "untyped")), "untyped root");
}

#[test]
fn microdata_value_matrix_uses_exact_raw_attributes_and_text_content() {
    let data = extract_structured_data(&StaticDocument::parse(fixture("microdata.html")));
    let values = data
        .iter()
        .find(|item| item.id.as_deref() == Some("../values"))
        .unwrap();
    let expected = [
        ("meta", "/meta"),
        ("audio", "/audio.ogg"),
        ("embed", "/embed.bin"),
        ("iframe", "/frame"),
        ("image", "/image.png"),
        ("source", "/source.mp4"),
        ("track", "/captions.vtt"),
        ("video", "/video.mp4"),
        ("anchor", "../link"),
        ("area", "#area"),
        ("link", "//cdn.example/x"),
        ("object", "data:text/plain,x"),
        ("data", "42"),
        ("meter", "0.5"),
        ("time", "2026-08-25"),
        ("generic-value", "A  B"),
    ];
    for (name, expected) in expected {
        assert_eq!(string(field(&values.value, name)), expected, "field {name}");
    }
    assert_eq!(
        values.value.get("image").and_then(StructuredValue::as_str),
        Some("/image.png")
    );
}
