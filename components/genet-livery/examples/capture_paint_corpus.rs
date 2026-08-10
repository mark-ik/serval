//! Capture real paint lists for netrender's regression corpus.
//!
//! ```text
//! cargo run -p genet-livery --example capture_paint_corpus -- <out-dir>
//! ```
//!
//! Runs HTML + CSS through the real pipeline (Livery cascade, Taffy
//! layout, Parley text) and writes each resulting `PaintEnvelope` as a
//! postcard-encoded `.paintlist`, plus a command histogram on stdout so
//! you can see which primitives a page actually exercised.
//!
//! The output files go in netrender's
//! `paint_list_render/tests/corpus/`, which replays them through
//! `translate_paint_list` and asserts the resulting `Scene` op stream
//! against a recorded golden. See that directory's README; fixtures need
//! a `.provenance` sidecar and a blessed `.ops` before they assert
//! anything.
//!
//! This exists because netrender's own tests are all scenes it wrote for
//! itself, which cannot catch a consumer driving the vocabulary in a
//! shape nobody anticipated. Roadmap A5.

use genet_livery::{Device, InteractionStates, StyleSet, emit_paint_list, layout, resolve_styles};
use genet_static_dom::StaticDocument;
use paint_list_api::{DeviceIntSize, PaintCmd, PaintEnvelope};

const WIDTH: f32 = 320.0;
const HEIGHT: f32 = 240.0;

fn render(html: &str, css: &str, generation: u64) -> PaintEnvelope {
    let document = StaticDocument::parse(html);
    let styles = resolve_styles(
        &document,
        &StyleSet::cambium(&[css]),
        &Device::screen(WIDTH, HEIGHT),
        &InteractionStates::default(),
    );
    let fragments = layout(&document, &styles, WIDTH, HEIGHT).unwrap();
    let list = emit_paint_list(
        &document,
        &styles,
        &fragments,
        DeviceIntSize::new(WIDTH as i32, HEIGHT as i32),
        generation,
    );
    PaintEnvelope::from_list(&list)
}

fn kind(cmd: &PaintCmd) -> &'static str {
    match cmd {
        PaintCmd::PushClip(_) => "PushClip",
        PaintCmd::PopClip => "PopClip",
        PaintCmd::PushTransform(_) => "PushTransform",
        PaintCmd::PopTransform => "PopTransform",
        PaintCmd::PushLayer(_) => "PushLayer",
        PaintCmd::PopLayer => "PopLayer",
        PaintCmd::DrawRect(_) => "DrawRect",
        PaintCmd::DrawStroke(_) => "DrawStroke",
        PaintCmd::DrawLine(_) => "DrawLine",
        PaintCmd::DrawPath(_) => "DrawPath",
        PaintCmd::DrawBorder(_) => "DrawBorder",
        PaintCmd::DrawLinearGradient(_) => "DrawLinearGradient",
        PaintCmd::DrawRadialGradient(_) => "DrawRadialGradient",
        PaintCmd::DrawConicGradient(_) => "DrawConicGradient",
        PaintCmd::DrawText(_) => "DrawText",
        PaintCmd::DrawImage(_) => "DrawImage",
        PaintCmd::DrawRepeatingImage(_) => "DrawRepeatingImage",
        PaintCmd::DrawExternalTexture(_) => "DrawExternalTexture",
        PaintCmd::DrawShadow(_) => "DrawShadow",
        PaintCmd::PushShadow(_) => "PushShadow",
        PaintCmd::PopAllShadows => "PopAllShadows",
        PaintCmd::HitTest(_) => "HitTest",
    }
}

fn histogram(envelope: &PaintEnvelope) -> Vec<(&'static str, usize)> {
    let mut counts: Vec<(&'static str, usize)> = Vec::new();
    for cmd in &envelope.commands {
        let k = kind(cmd);
        match counts.iter_mut().find(|(name, _)| *name == k) {
            Some((_, n)) => *n += 1,
            None => counts.push((k, 1)),
        }
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    counts
}

/// A text-and-borders article card. Aimed at the primitives netrender's
/// hand-written corpus seeds do not reach: shaped text runs and CSS
/// borders with radii.
const ARTICLE_HTML: &str = r#"<html><body>
<div class="card">
  <h1>Paint lists</h1>
  <p>A renderer that ingests display lists should be tested with display
  lists someone else produced.</p>
  <p class="note">Captured through Livery, Taffy and Parley.</p>
</div>
</body></html>"#;

const ARTICLE_CSS: &str = r#"
body { background: #f4f4f7; margin: 8px; font-family: sans-serif; }
.card {
  background: #ffffff;
  border: 2px solid #3355aa;
  border-radius: 8px;
  padding: 12px;
}
h1 { font-size: 20px; color: #14203a; margin: 0 0 8px 0; }
p { font-size: 12px; color: #3a3a44; margin: 0 0 6px 0; }
.note {
  font-size: 10px;
  color: #6a6a78;
  border-top: 1px dashed #ccccd8;
  padding-top: 6px;
}
"#;

/// Nested overflow + per-side borders: the box-model shapes that produce
/// clip scopes and non-uniform `DrawBorder` details.
const NESTED_HTML: &str = r#"<html><body>
<div class="outer">
  <div class="row"><span>alpha</span></div>
  <div class="row wide"><span>beta</span></div>
  <div class="row"><span>gamma</span></div>
</div>
</body></html>"#;

const NESTED_CSS: &str = r#"
body { background: #101018; margin: 0; }
.outer {
  margin: 10px;
  padding: 6px;
  background: #1c1c28;
  border-left: 4px solid #cc4466;
  border-bottom: 1px solid #444458;
  overflow: hidden;
  height: 120px;
}
.row {
  background: #262636;
  border-radius: 3px;
  margin-bottom: 4px;
  padding: 4px 6px;
  height: 18px;
}
.wide { background: #303048; border: 1px solid #5a5a7a; }
span { font-size: 11px; color: #d8d8e4; }
"#;

/// Replace font and image payloads with a short deterministic stand-in.
///
/// `FontResource` carries raw TTF/OTF bytes inline — its own doc notes
/// fonts run 100 KB to 20 MB — so a faithful capture of any page with
/// text is multi-megabyte, which is no good for a corpus meant to
/// accumulate fixtures in git. The command stream is what the corpus
/// tests, and it is preserved exactly: keys, glyph ids, positions,
/// colours and every other field are untouched. Only the opaque payload
/// each key resolves to is shortened.
///
/// This is verified rather than assumed: capturing with `--keep-payloads`
/// and blessing produces byte-identical `.ops` goldens.
///
/// The cost is that an elided fixture cannot be rasterized — the font
/// bytes are not a real face. That is fine for the CPU-tier corpus and
/// is called out in its README as a constraint on any future GPU tier.
fn elide_payloads(envelope: &mut PaintEnvelope) -> (usize, usize) {
    let mut before = 0;
    for font in &mut envelope.fonts {
        before += font.data.len();
        font.data = std::sync::Arc::new(b"<elided font payload>".to_vec());
    }
    for image in &mut envelope.images {
        before += image.data.len();
        // Keep width/height honest by shrinking to a 1x1 RGBA pixel.
        image.width = 1;
        image.height = 1;
        image.data = vec![0, 0, 0, 0];
    }
    let after = envelope.fonts.iter().map(|f| f.data.len()).sum::<usize>()
        + envelope.images.iter().map(|i| i.data.len()).sum::<usize>();
    (before, after)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(out_dir) = args.next() else {
        eprintln!("usage: capture_paint_corpus <out-dir> [--keep-payloads]");
        std::process::exit(2);
    };
    let keep_payloads = args.any(|a| a == "--keep-payloads");
    let out_dir = std::path::PathBuf::from(out_dir);
    std::fs::create_dir_all(&out_dir).expect("create out dir");

    let pages: Vec<(&str, PaintEnvelope)> = vec![
        ("livery_article_card", render(ARTICLE_HTML, ARTICLE_CSS, 1)),
        ("livery_nested_rows", render(NESTED_HTML, NESTED_CSS, 2)),
    ];

    for (name, mut envelope) in pages {
        let elided = if keep_payloads {
            None
        } else {
            Some(elide_payloads(&mut envelope))
        };

        let bytes = postcard::to_allocvec(&envelope).expect("encode envelope");
        let path = out_dir.join(format!("{name}.paintlist"));
        std::fs::write(&path, &bytes).expect("write fixture");

        println!(
            "{name}.paintlist  {} commands, {} fonts, {} images, {} bytes",
            envelope.commands.len(),
            envelope.fonts.len(),
            envelope.images.len(),
            bytes.len()
        );
        if let Some((before, after)) = elided {
            println!("    payloads elided: {before} -> {after} bytes");
        }
        for (kind, n) in histogram(&envelope) {
            println!("    {n:>4}  {kind}");
        }
    }
}
