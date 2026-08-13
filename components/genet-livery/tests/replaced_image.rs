use genet_livery::{Device, InteractionStates, StyleSet, emit_paint_list, layout, resolve_styles};
use genet_static_dom::StaticDocument;
use layout_dom_api::{LayoutDom, LocalName, Namespace};
use livery::values::{AspectRatio, Length, LengthPercentage, Size};
use paint_list_api::{DeviceIntSize, PaintCmd, PaintList};

#[test]
fn html_image_dimensions_reach_paint_through_computed_css() {
    use base64::Engine as _;

    let image = image::RgbaImage::from_pixel(2, 3, image::Rgba([0, 0, 255, 255]));
    let mut png = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .expect("encode test PNG");
    let data_uri = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    );
    let document = StaticDocument::parse(&format!(
        r#"<html><body><img src="{data_uri}" width="20" height="96"></body></html>"#
    ));
    let styles = resolve_styles(
        &document,
        &StyleSet::cambium(&["body { margin: 0; }"]),
        &Device::screen(320.0, 240.0),
        &InteractionStates::default(),
    );
    let image_node = document
        .dom_children(document.document())
        .flat_map(|html| document.dom_children(html))
        .flat_map(|body| document.dom_children(body))
        .find(|node| {
            document
                .element_name(*node)
                .is_some_and(|name| name.local.as_ref() == "img")
        })
        .expect("image node");
    let computed = styles.get(image_node).expect("computed image style");
    assert_eq!(
        computed.width,
        Size::Value(LengthPercentage::Length(Length::px(20.0)))
    );
    assert_eq!(
        computed.height,
        Size::Value(LengthPercentage::Length(Length::px(96.0)))
    );
    assert_eq!(
        computed.aspect_ratio,
        AspectRatio::AutoRatio {
            width: 20.0,
            height: 96.0,
        }
    );
    let fragments = layout(&document, &styles, 320.0, 240.0).expect("layout image");
    let list = emit_paint_list(
        &document,
        &styles,
        &fragments,
        DeviceIntSize::new(320, 240),
        1,
    );
    let PaintCmd::DrawImage(image) = list
        .commands()
        .iter()
        .find(|command| matches!(command, PaintCmd::DrawImage(_)))
        .expect("replaced image paints")
    else {
        unreachable!();
    };
    assert_eq!(image.placement.bounds.size().width, 20.0);
    assert_eq!(image.placement.bounds.size().height, 96.0);
}

#[test]
fn authored_auto_axis_uses_the_decoded_natural_ratio_not_the_attribute_ratio() {
    use base64::Engine as _;

    let image = image::RgbaImage::from_pixel(2, 3, image::Rgba([0, 0, 255, 255]));
    let mut png = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .expect("encode test PNG");
    let data_uri = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    );
    let document = StaticDocument::parse(&format!(
        r#"<html><body><img id="image" src="{data_uri}" width="20" height="96"></body></html>"#
    ));
    let styles = resolve_styles(
        &document,
        &StyleSet::cambium(&["body { margin: 0; } #image { width: 40px; height: auto; }"]),
        &Device::screen(320.0, 240.0),
        &InteractionStates::default(),
    );
    let image_node = document
        .dom_children(document.document())
        .flat_map(|html| document.dom_children(html))
        .flat_map(|body| document.dom_children(body))
        .find(|node| {
            document.attribute(node, &Namespace::from(""), &LocalName::from("id")) == Some("image")
        })
        .expect("image node");
    let computed = styles.get(image_node).expect("computed image style");
    assert_eq!(
        computed.width,
        Size::Value(LengthPercentage::Length(Length::px(40.0)))
    );
    assert_eq!(computed.height, Size::Auto);
    assert_eq!(
        computed.aspect_ratio,
        AspectRatio::AutoRatio {
            width: 20.0,
            height: 96.0,
        },
        "the hint remains visible in computed CSS"
    );

    let fragments = layout(&document, &styles, 320.0, 240.0).expect("layout image");
    let fragment = fragments
        .get(image_node)
        .expect("image fragment")
        .physical_rect();
    assert_eq!(fragment.width, 40.0);
    assert_eq!(fragment.height, 60.0);
}
