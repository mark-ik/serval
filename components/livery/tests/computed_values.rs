use livery::values::{
    Alignment, Color, FlexBasis, FontWeight, Length, LengthPercentage, Opacity, Size, Transform,
};
use livery::{ComputedValues, PropertyId, ValueType, canonicalize_specified_longhand};

#[test]
fn generated_initial_values_match_the_catalog() {
    let values = ComputedValues::default();

    assert_eq!(values.color, Color::CANVAS_TEXT);
    assert_eq!(values.background_color, Color::TRANSPARENT);
    assert_eq!(values.width, Size::Auto);
    assert_eq!(values.height, Size::Auto);
    assert_eq!(values.flex_basis, FlexBasis::Auto);
    assert_eq!(values.font_weight, FontWeight::Normal);
    assert_eq!(values.opacity, Opacity::ONE);
    assert_eq!(values.transform, Transform::None);
    assert_eq!(values.justify_content, Alignment::Normal);
    assert_eq!(values.z_index.to_string(), "auto");
    assert_eq!(
        canonicalize_specified_longhand("justify-content", "normal").as_deref(),
        Some("normal")
    );
}

#[test]
fn child_values_copy_only_inherited_fields() {
    let parent = ComputedValues {
        color: "#3568b8".parse().unwrap(),
        font_weight: FontWeight::Number(700),
        width: Size::Value(LengthPercentage::Length(Length::rem(42.0))),
        background_color: "#ffffff".parse().unwrap(),
        ..ComputedValues::default()
    };

    let child = ComputedValues::for_child(&parent);

    assert_eq!(child.color, parent.color);
    assert_eq!(child.font_weight, parent.font_weight);
    assert_eq!(child.width, Size::Auto);
    assert_eq!(child.background_color, Color::TRANSPARENT);
}

#[test]
fn generated_metadata_names_the_concrete_value_family() {
    assert_eq!(
        PropertyId::BackgroundColor.metadata().value_type,
        ValueType::Color
    );
    assert_eq!(PropertyId::Width.metadata().value_type, ValueType::Size);
    assert_eq!(
        PropertyId::TextDecorationLine.metadata().value_type,
        ValueType::TextDecorationLine
    );
}
