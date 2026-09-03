// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! The reftest lane: reference resolution, fuzzy-match policy, image
//! comparison, and the table ledger a reftest run publishes.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReftestRenderer {
    Livery,
}

impl ReftestRenderer {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "livery" => Some(Self::Livery),
            _ => None,
        }
    }

    pub(crate) fn harness_style(self) -> harness::StyleRoute {
        harness::StyleRoute::Livery
    }

    pub(crate) fn label(self) -> &'static str {
        "livery"
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MatchKind {
    Match,
    Mismatch,
}

/// The first `<link rel="match"|"mismatch" href="...">` in a reftest.
pub(crate) fn reftest_ref(html: &str) -> Option<(MatchKind, String)> {
    let doc = StaticDocument::parse_auto(html);
    let no_ns = layout_dom_api::Namespace::default();
    let rel = LocalName::from("rel");
    let href = LocalName::from("href");
    let mut stack = vec![doc.document()];
    while let Some(id) = stack.pop() {
        if doc
            .element_name(id)
            .is_some_and(|q| q.local.as_ref() == "link")
        {
            let kind = match doc.attribute(id, &no_ns, &rel) {
                Some("match") => Some(MatchKind::Match),
                Some("mismatch") => Some(MatchKind::Mismatch),
                _ => None,
            };
            if let Some(kind) = kind {
                if let Some(h) = doc.attribute(id, &no_ns, &href) {
                    return Some((kind, h.to_string()));
                }
            }
        }
        stack.extend(doc.dom_children(id));
    }
    None
}

/// Skip reftests whose pixels depend on script having run. Inline + linked CSS
/// and local images are loaded; remote resources just render as missing.
///
/// Reftests render with no JS engine, so the question is not "is there script"
/// but "would running it change the screenshot". A document with no `<script>`
/// at all is runnable, as before. One with script is runnable only if every
/// script is a property read (`document.body.offsetTop;`) and nothing else in
/// the document can hand script a way in. That read is a WPT idiom for forcing
/// a style/layout flush partway through parsing, so the flushed and unflushed
/// renders are the same picture; the tests using it are otherwise static.
///
/// Everything ambiguous skips. A script we cannot read (`src=`), a tag we
/// cannot pair with its close, an event handler we will not fire, and
/// `reftest-wait` (WPT's contract that the screenshot waits for script) all
/// count as needing script.
pub(crate) fn needs_script(html: &str) -> bool {
    let lower = html.to_ascii_lowercase();
    let mut saw_script = false;
    let mut cursor = 0;
    // Lexical, not DOM: a `<script` inside a comment or in text counts as a
    // script here. That over-counts, which is the safe direction.
    while let Some(offset) = lower[cursor..].find("<script") {
        saw_script = true;
        let open = cursor + offset;
        let Some(tag_end) = lower[open..].find('>').map(|i| open + i) else {
            return true;
        };
        if has_attribute(&lower[open..tag_end], "src") {
            return true;
        }
        let body = tag_end + 1;
        let Some(close) = lower[body..].find("</script").map(|i| body + i) else {
            return true;
        };
        if !is_read_only_script(&html[body..close]) {
            return true;
        }
        cursor = close;
    }
    if !saw_script {
        return false;
    }
    // Inert script elements settle only the script elements. These two say the
    // document expects script to run regardless of what the elements contain.
    lower.contains("reftest-wait") || has_event_handler_attribute(&lower)
}

/// A script body that can only read: every statement is a bare property access,
/// with no call, assignment, operator, or control flow. Reading a DOM property
/// paints nothing, and a body restricted to reads cannot have installed a
/// getter that would.
pub(crate) fn is_read_only_script(body: &str) -> bool {
    let body = body.trim();
    // XHTML tests wrap script bodies in CDATA; the wrapper is syntax, not code.
    let body = body
        .strip_prefix("<![CDATA[")
        .and_then(|b| b.strip_suffix("]]>"))
        .unwrap_or(body);
    body.split(';')
        .map(str::trim)
        .all(|statement| statement.is_empty() || is_property_path(statement))
}

/// `foo`, `document.body.offsetTop` — an identifier path and nothing else.
pub(crate) fn is_property_path(text: &str) -> bool {
    !text.is_empty()
        && text.split('.').all(|segment| {
            let mut chars = segment.chars();
            chars
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        })
}

/// An `on…=` attribute anywhere in the lowercased source: an entry point this
/// runner will not fire, so whatever it would have painted is not what we
/// screenshot.
pub(crate) fn has_event_handler_attribute(lower: &str) -> bool {
    let bytes = lower.as_bytes();
    let mut cursor = 0;
    while let Some(offset) = lower[cursor..].find("on") {
        let start = cursor + offset;
        cursor = start + 2;
        // An attribute name starts after whitespace inside a tag.
        if start == 0 || !bytes[start - 1].is_ascii_whitespace() {
            continue;
        }
        let mut end = cursor;
        while bytes.get(end).is_some_and(u8::is_ascii_lowercase) {
            end += 1;
        }
        if end == cursor {
            continue;
        }
        if attribute_value_follows(bytes, end) {
            return true;
        }
    }
    false
}

/// Whether `name` appears as an attribute name in a lowercased open tag.
pub(crate) fn has_attribute(tag: &str, name: &str) -> bool {
    let bytes = tag.as_bytes();
    let mut cursor = 0;
    while let Some(offset) = tag[cursor..].find(name) {
        let start = cursor + offset;
        cursor = start + name.len();
        if start > 0
            && bytes[start - 1].is_ascii_whitespace()
            && attribute_value_follows(bytes, cursor)
        {
            return true;
        }
    }
    false
}

pub(crate) fn attribute_value_follows(bytes: &[u8], mut at: usize) -> bool {
    while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
        at += 1;
    }
    bytes.get(at) == Some(&b'=')
}

/// WPT `<meta name="fuzzy" content="...">` tolerance, as
/// inclusive `(max_per_channel_difference, max_differing_pixels)` ranges.
/// Common forms: `maxDifference=0-2;totalPixels=0-100` or `0-2;0-100`.
pub(crate) fn parse_fuzzy(html: &str) -> Option<FuzzyRange> {
    let doc = StaticDocument::parse_auto(html);
    let no_ns = layout_dom_api::Namespace::default();
    let name = LocalName::from("name");
    let content = LocalName::from("content");
    let mut stack = vec![doc.document()];
    while let Some(id) = stack.pop() {
        if doc
            .element_name(id)
            .is_some_and(|q| q.local.as_ref() == "meta")
            && doc.attribute(id, &no_ns, &name) == Some("fuzzy")
        {
            if let Some(c) = doc.attribute(id, &no_ns, &content) {
                return parse_fuzzy_content(c);
            }
        }
        stack.extend(doc.dom_children(id));
    }
    None
}

pub(crate) fn parse_fuzzy_content(content: &str) -> Option<FuzzyRange> {
    let values = content
        .trim()
        .rsplit_once(':')
        .map_or(content.trim(), |(_, values)| values);
    let mut positional = Vec::new();
    let mut max_difference = None;
    let mut total_pixels = None;
    for segment in values.split(';') {
        let (name, value) = segment
            .split_once('=')
            .map_or((None, segment), |(name, value)| (Some(name.trim()), value));
        let range = parse_fuzzy_range(value)?;
        match name {
            Some("maxDifference") if max_difference.is_none() => max_difference = Some(range),
            Some("totalPixels") if total_pixels.is_none() => total_pixels = Some(range),
            Some(_) => return None,
            None => positional.push(range),
        }
    }
    let mut positional = positional.into_iter();
    let difference = max_difference.or_else(|| positional.next())?;
    let pixels = total_pixels.or_else(|| positional.next())?;
    if positional.next().is_some() {
        return None;
    }
    Some((
        (
            difference.0.min(u64::from(u16::MAX)) as u16,
            difference.1.min(u64::from(u16::MAX)) as u16,
        ),
        pixels,
    ))
}

pub(crate) fn parse_fuzzy_range(value: &str) -> Option<(u64, u64)> {
    let value = value.trim();
    let (lo, hi) = value.split_once('-').unwrap_or((value, value));
    let lo = lo.trim().parse().ok()?;
    let hi = hi.trim().parse().ok()?;
    (lo <= hi).then_some((lo, hi))
}

/// Whether two images match under an optional fuzzy tolerance. With
/// `None`, exact. With a range, the observed largest channel delta and number
/// of pixels with any non-zero delta must fall within their independent WPT
/// ranges. WPT treats a zero observation as acceptable when that metric's
/// lower bound is zero, even if the other metric has a positive lower bound.
pub(crate) fn images_match(
    a: &render::Image,
    b: &render::Image,
    fuzzy: Option<FuzzyRange>,
) -> bool {
    if a.dimensions() != b.dimensions() {
        return false;
    }
    let mut pixels_different = 0u64;
    let mut max_per_channel = 0u16;
    for (pa, pb) in a.pixels().zip(b.pixels()) {
        let channel_max =
            pa.0.iter()
                .zip(pb.0.iter())
                .map(|(x, y)| (i16::from(*x) - i16::from(*y)).unsigned_abs())
                .max()
                .unwrap_or(0);
        if channel_max != 0 {
            pixels_different += 1;
            max_per_channel = max_per_channel.max(channel_max);
        }
    }
    let Some(((difference_lo, difference_hi), (pixels_lo, pixels_hi))) = fuzzy else {
        return pixels_different == 0 && max_per_channel == 0;
    };
    (pixels_different == 0 && pixels_lo == 0)
        || (max_per_channel == 0 && difference_lo == 0)
        || ((difference_lo..=difference_hi).contains(&max_per_channel)
            && (pixels_lo..=pixels_hi).contains(&pixels_different))
}

pub(crate) fn widen_fuzzy_for_gpu(
    fuzzy: Option<FuzzyRange>,
    viewport: render::RenderViewport,
) -> FuzzyRange {
    let floor_pixels = fuzz_floor_pixels(viewport);
    fuzzy.map_or(
        ((0, FUZZ_FLOOR_DIFF), (0, floor_pixels)),
        |((difference_lo, difference_hi), (pixels_lo, pixels_hi))| {
            (
                (difference_lo, difference_hi.max(FUZZ_FLOOR_DIFF)),
                (pixels_lo, pixels_hi.max(floor_pixels)),
            )
        },
    )
}

pub(crate) const CHECKED_REFERENCE_VERIFICATION: &str =
    include_str!("../expectations/reftest/reference_verification.json");

#[derive(serde::Deserialize)]
pub(crate) struct ReferenceVerificationFile {
    pub(crate) version: u32,
    pub(crate) renderer: String,
    pub(crate) reason: String,
    pub(crate) source: String,
    pub(crate) scopes: Vec<String>,
    pub(crate) tests: Vec<String>,
}

pub(crate) struct ReferenceVerification {
    pub(crate) reason: String,
    pub(crate) source: String,
    pub(crate) scopes: Vec<String>,
    pub(crate) tests: BTreeSet<String>,
}

impl ReferenceVerification {
    pub(crate) fn load(path: Option<&Path>, renderer: &str) -> Result<Self, String> {
        let (label, contents) = match path {
            Some(path) => (
                path.display().to_string(),
                fs::read_to_string(path).map_err(|error| {
                    format!(
                        "reference verification read failed ({}): {error}",
                        path.display()
                    )
                })?,
            ),
            None => (
                "checked-in reference_verification.json".to_string(),
                CHECKED_REFERENCE_VERIFICATION.to_string(),
            ),
        };
        Self::parse(&contents, &label, renderer)
    }

    pub(crate) fn parse(contents: &str, label: &str, renderer: &str) -> Result<Self, String> {
        let file: ReferenceVerificationFile = serde_json::from_str(contents)
            .map_err(|error| format!("reference verification parse failed ({label}): {error}"))?;
        if file.version != 1 {
            return Err(format!(
                "reference verification {label} has version {}, expected 1",
                file.version
            ));
        }
        if !file.renderer.eq_ignore_ascii_case(renderer) {
            return Err(format!(
                "reference verification {label} records renderer `{}`, but this run uses `{renderer}`",
                file.renderer
            ));
        }
        if file.reason != "reference-unverified" || file.source.trim().is_empty() {
            return Err(format!(
                "reference verification {label} needs reason `reference-unverified` and a non-empty `source`"
            ));
        }
        let mut scopes = BTreeSet::new();
        for scope in file.scopes {
            validate_reference_verification_path(&scope, true, label)?;
            if !scopes.insert(scope.clone()) {
                return Err(format!(
                    "reference verification {label} repeats scope `{scope}`"
                ));
            }
        }
        let mut tests = BTreeSet::new();
        for test in file.tests {
            validate_reference_verification_path(&test, false, label)?;
            if !tests.insert(test.clone()) {
                return Err(format!(
                    "reference verification {label} repeats test `{test}`"
                ));
            }
        }
        Ok(Self {
            reason: file.reason,
            source: file.source,
            scopes: scopes.into_iter().collect(),
            tests,
        })
    }

    pub(crate) fn reason_for(&self, test: &str) -> Option<&str> {
        (self.tests.contains(test) || self.scopes.iter().any(|scope| test.starts_with(scope)))
            .then_some(self.reason.as_str())
    }
}

pub(crate) fn validate_reference_verification_path(
    path: &str,
    scope: bool,
    label: &str,
) -> Result<(), String> {
    let shape_is_valid = !path.is_empty()
        && path.trim() == path
        && !path.starts_with('/')
        && !path.contains('\\')
        && path.ends_with('/') == scope;
    if shape_is_valid {
        Ok(())
    } else {
        Err(format!(
            "reference verification {label} has invalid {} `{path}`",
            if scope { "scope" } else { "test" }
        ))
    }
}

/// Full per-pixel diff between a test render and its reference. The shape of a
/// failure buckets it (Lever 2 diagnosis): `differing == total` with a large
/// `max_channel_diff` → whole render diverges (layout/parse/UA-stylesheet);
/// `max_channel_diff` small with many `differing` → anti-aliasing / sub-pixel
/// (a fuzzy-tolerance case); `max_channel_diff` large but `differing` localized →
/// a specific paint/feature gap. `!same_dims` → a sizing divergence before paint.
pub(crate) struct DiffStats {
    pub(crate) same_dims: bool,
    pub(crate) differing: u64,
    pub(crate) total: u64,
    pub(crate) max_channel_diff: u16,
}

pub(crate) fn diff_stats(a: &render::Image, b: &render::Image) -> DiffStats {
    let total = u64::from(a.width()) * u64::from(a.height());
    if a.dimensions() != b.dimensions() {
        return DiffStats {
            same_dims: false,
            differing: total,
            total,
            max_channel_diff: 255,
        };
    }
    let (mut differing, mut max_channel_diff) = (0u64, 0u16);
    for (pa, pb) in a.pixels().zip(b.pixels()) {
        let channel_max =
            pa.0.iter()
                .zip(pb.0.iter())
                .map(|(x, y)| (i16::from(*x) - i16::from(*y)).unsigned_abs())
                .max()
                .unwrap_or(0);
        if channel_max > 0 {
            differing += 1;
            max_channel_diff = max_channel_diff.max(channel_max);
        }
    }
    DiffStats {
        same_dims: true,
        differing,
        total,
        max_channel_diff,
    }
}

/// One-line classification of a FAIL's diff shape, for `-v` triage.
pub(crate) fn diff_label(s: &DiffStats) -> &'static str {
    if !s.same_dims {
        "dims" // different output size — layout/sizing divergence pre-paint
    } else if s.differing == 0 {
        "equal?" // identical yet failed match — a harness/tolerance quirk
    } else if s.total > 0 && s.differing * 100 / s.total >= 50 {
        "whole" // >=50% of pixels differ — wholesale (layout / UA stylesheet)
    } else if s.max_channel_diff <= 16 {
        "aa" // small per-channel diffs — anti-aliasing / sub-pixel
    } else {
        "local" // localized large diffs — a specific paint/feature gap
    }
}

/// Follow a `match` ref chain to its final reference, returning that
/// reference's path + HTML. `mismatch` chains are not followed (the direct
/// reference is used). Capped to avoid cycles.
pub(crate) fn final_ref(
    start: PathBuf,
    kind: MatchKind,
    tests_root: &Path,
) -> Option<(PathBuf, String)> {
    let mut ref_path = start;
    let mut html = String::from_utf8_lossy(&fs::read(&ref_path).ok()?).into_owned();
    if kind == MatchKind::Mismatch {
        return Some((ref_path, html));
    }
    for _ in 0..10 {
        match reftest_ref(&html) {
            Some((MatchKind::Match, next_href)) => {
                if let Some(reference) = about_blank_reference(&next_href, tests_root) {
                    return Some(reference);
                }
                let Some(next) = resolve_ref(&ref_path, &next_href, tests_root) else {
                    break;
                };
                let Ok(bytes) = fs::read(&next) else { break };
                ref_path = next;
                html = String::from_utf8_lossy(&bytes).into_owned();
            },
            _ => break,
        }
    }
    Some((ref_path, html))
}

pub(crate) const ABOUT_BLANK_DOCUMENT: &str = "<!doctype html><meta charset=\"utf-8\">";

/// Represent WPT's built-in empty document without treating it as a file under
/// the test root. The synthetic path gives the renderer the test root as its
/// resource directory and keeps it on the ordinary HTML path.
pub(crate) fn about_blank_reference(href: &str, tests_root: &Path) -> Option<(PathBuf, String)> {
    let href = href.split(['#', '?']).next().unwrap_or(href);
    href.eq_ignore_ascii_case("about:blank").then(|| {
        (
            tests_root.join("__genet_wpt_about_blank__.html"),
            ABOUT_BLANK_DOCUMENT.into(),
        )
    })
}

/// Resolve a reftest reference, including WPT's built-in empty document.
pub(crate) fn resolve_reftest_reference(
    test_path: &Path,
    href: &str,
    kind: MatchKind,
    tests_root: &Path,
) -> Result<Option<(PathBuf, String)>, ()> {
    if let Some(reference) = about_blank_reference(href, tests_root) {
        return Ok(Some(reference));
    }
    let Some(direct_ref) = resolve_ref(test_path, href, tests_root) else {
        return Err(());
    };
    Ok(final_ref(direct_ref, kind, tests_root))
}

/// Resolve a reftest `href` to a file: `/`-absolute against the tests
/// root, otherwise relative to the test's directory. Drops fragment/query.
pub(crate) fn resolve_ref(test_path: &Path, href: &str, tests_root: &Path) -> Option<PathBuf> {
    let href = href.split(['#', '?']).next().unwrap_or(href);
    if href.is_empty() {
        return None;
    }
    Some(match href.strip_prefix('/') {
        Some(rest) => tests_root.join(rest),
        None => test_path.parent()?.join(href),
    })
}

/// Aggregate the exact dispatch records from documents the Livery reftest
/// renderer completed. This is deliberately separate from reftest pass/fail:
/// a fallback can still paint the expected pixels, so pixels cannot stand in
/// for ownership accounting.
#[derive(Default)]
pub(crate) struct TableLedgerSummary {
    pub(crate) documents: usize,
    pub(crate) inline_assigned: usize,
    pub(crate) inline_verified: usize,
    pub(crate) inline_honored: usize,
    pub(crate) collapsed_metrics: usize,
    pub(crate) inline_divergences: usize,
    pub(crate) inline_skips: BTreeMap<String, usize>,
    pub(crate) block_laid_out: usize,
    pub(crate) block_relaid_out: usize,
    pub(crate) block_verified: usize,
    pub(crate) block_agreed: usize,
    pub(crate) block_divergences: usize,
    pub(crate) block_skips: BTreeMap<String, usize>,
}

impl TableLedgerSummary {
    pub(crate) fn record(&mut self, ledger: &TableShadowLedger) {
        self.documents += 1;
        self.inline_assigned += ledger.assigned;
        self.inline_verified += ledger.verified;
        self.inline_honored += ledger.honored;
        self.collapsed_metrics += ledger.collapsed_metrics;
        self.inline_divergences += ledger.divergences.len();
        for (_, skip) in &ledger.skipped {
            *self
                .inline_skips
                .entry(shadow_skip_label(skip))
                .or_default() += 1;
        }

        let block = &ledger.block;
        self.block_laid_out += block.laid_out;
        self.block_relaid_out += block.relaid_out;
        self.block_verified += block.verified;
        self.block_agreed += block.agreed;
        self.block_divergences += block.divergences.len();
        for (_, skip) in &block.skipped {
            *self.block_skips.entry(block_skip_label(skip)).or_default() += 1;
        }
    }
}

pub(crate) fn variant_label(value: &impl std::fmt::Debug) -> String {
    let debug = format!("{value:?}");
    debug
        .split(['(', '{'])
        .next()
        .unwrap_or(&debug)
        .trim_end()
        .to_owned()
}

pub(crate) fn shadow_skip_label(skip: &TableShadowSkip) -> String {
    match skip {
        TableShadowSkip::Deferred(deferral) => format!("Deferred::{deferral:?}"),
        TableShadowSkip::AutomaticIncompleteCells => "AutomaticIncompleteCells".to_owned(),
        TableShadowSkip::AutomaticIndefinite(reason) => {
            format!("AutomaticIndefinite::{}", variant_label(reason))
        },
        TableShadowSkip::CollapsedBorder(error) => {
            format!("CollapsedBorder::{}", variant_label(error))
        },
        TableShadowSkip::Error(error) => format!("Error::{}", variant_label(error)),
    }
}

pub(crate) fn block_skip_label(skip: &TableBlockSkip) -> String {
    match skip {
        TableBlockSkip::Deferred(deferral) => format!("Deferred::{deferral:?}"),
        TableBlockSkip::DeferredInLowering(error) => {
            format!("DeferredInLowering::{}", variant_label(error))
        },
        TableBlockSkip::IncompleteCells => "IncompleteCells".to_owned(),
        TableBlockSkip::IncompleteRowGroup => "IncompleteRowGroup".to_owned(),
        TableBlockSkip::Error(error) => format!("Error::{}", variant_label(error)),
    }
}

pub(crate) fn write_table_ledger(
    path: &Path,
    summary: &TableLedgerSummary,
) -> Result<(), std::io::Error> {
    let mut out = String::from("# genet-wpt Livery table dispatch census\n");
    out.push_str(&format!("rendered-documents: {}\n\n", summary.documents));
    out.push_str("## inline\n");
    out.push_str(&format!("assigned: {}\n", summary.inline_assigned));
    out.push_str(&format!("verified: {}\n", summary.inline_verified));
    out.push_str(&format!("honored: {}\n", summary.inline_honored));
    out.push_str(&format!(
        "collapsed-metrics: {}\n",
        summary.collapsed_metrics
    ));
    out.push_str(&format!("divergences: {}\n", summary.inline_divergences));
    out.push_str("skips:\n");
    for (label, count) in &summary.inline_skips {
        out.push_str(&format!("  {label}: {count}\n"));
    }
    out.push_str("\n## block\n");
    out.push_str(&format!("laid-out: {}\n", summary.block_laid_out));
    out.push_str(&format!("relaid-out: {}\n", summary.block_relaid_out));
    out.push_str(&format!("verified: {}\n", summary.block_verified));
    out.push_str(&format!("agreed: {}\n", summary.block_agreed));
    out.push_str(&format!("divergences: {}\n", summary.block_divergences));
    out.push_str("skips:\n");
    for (label, count) in &summary.block_skips {
        out.push_str(&format!("  {label}: {count}\n"));
    }
    fs::write(path, out)
}

pub(crate) fn reftest(tests: &[TestCase], args: &Args) {
    let reference_verification = match ReferenceVerification::load(
        args.reference_verification.as_deref(),
        args.renderer.label(),
    ) {
        Ok(verification) => verification,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        },
    };
    let renderer = match render::Renderer::boot() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("cannot boot renderer (reftests need a GPU): {e}");
            std::process::exit(1);
        },
    };
    let viewport = render::RenderViewport::new(REFTEST_W, REFTEST_H, args.device_scale)
        .expect("parse_args validates --device-scale");
    let tests_root = Path::new(&args.tests_root);

    let prev = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));

    let (mut passed, mut reference_unverified, mut failed, mut skipped, mut errored) =
        (0, 0, 0, 0, 0);
    let mut buckets: HashMap<&'static str, u64> = HashMap::new();
    let mut table_ledger = args
        .write_table_ledger
        .as_ref()
        .map(|_| TableLedgerSummary::default());
    // Per-test coarse status for the optional expectations baseline (mirrors the
    // testharness lane's `ActualRecord`s), fed to `finish_expectations` below.
    let mut actuals: Vec<ActualRecord> = Vec::new();
    for test in tests {
        let Ok(bytes) = fs::read(&test.path) else {
            errored += 1;
            actuals.push(ActualRecord::with_reason(test, "error", "read-failed"));
            continue;
        };
        let test_html = String::from_utf8_lossy(&bytes).into_owned();
        if test.kind != Kind::Reftest {
            skipped += 1;
            actuals.push(ActualRecord::with_reason(test, "skip", "non-reftest"));
            continue;
        }
        let (kind, href) = if let Some((href, cmp)) = test.refs.first() {
            let kind = match cmp {
                manifest::RefMatch::Equal => MatchKind::Match,
                manifest::RefMatch::NotEqual => MatchKind::Mismatch,
            };
            (kind, href.clone())
        } else {
            let Some((kind, href)) = reftest_ref(&test_html) else {
                skipped += 1;
                actuals.push(ActualRecord::with_reason(test, "skip", "no-ref"));
                continue;
            };
            (kind, href)
        };
        let reference = match resolve_reftest_reference(&test.path, &href, kind, tests_root) {
            Ok(Some(reference)) => reference,
            Ok(None) => {
                errored += 1;
                actuals.push(ActualRecord::with_reason(test, "error", "ref-missing"));
                println!("ERROR ref-missing {}", test.name());
                continue;
            },
            Err(()) => {
                skipped += 1;
                actuals.push(ActualRecord::with_reason(test, "skip", "ref-unresolved"));
                continue;
            },
        };
        let (ref_path, ref_html) = reference;
        if needs_script(&test_html) || needs_script(&ref_html) {
            skipped += 1;
            actuals.push(ActualRecord::with_reason(test, "skip", "needs-script"));
            if args.verbose {
                println!("SKIP  script   {}", test.name());
            }
            continue;
        }

        // Apply the GPU-jitter floor (see FUZZ_FLOOR_*): never compare tighter
        // than its upper bounds. Keep WPT's authored lower bounds intact.
        let fuzzy = widen_fuzzy_for_gpu(test.fuzzy.or_else(|| parse_fuzzy(&test_html)), viewport);
        let test_dir = test.path.parent().unwrap_or(tests_root);
        let ref_dir = ref_path.parent().unwrap_or(tests_root);
        let test_xml = is_xml_path(&test.path);
        let ref_xml = is_xml_path(&ref_path);
        let rendered = panic::catch_unwind(AssertUnwindSafe(|| {
            let render = |html: &str, dir: &Path, xml: bool| {
                let rendered = renderer.render_html(html, dir, tests_root, viewport, xml);
                (rendered.image, Some(rendered.table_ledger))
            };
            let t = render(&test_html, test_dir, test_xml);
            let r = render(&ref_html, ref_dir, ref_xml);
            (t, r)
        }));
        let ((test_img, test_ledger), (ref_img, ref_ledger)) = match rendered {
            Ok(pair) => pair,
            Err(_) => {
                failed += 1;
                actuals.push(ActualRecord::with_reason(test, "fail", "crash"));
                println!("FAIL  crash    {}", test.name());
                continue;
            },
        };
        if let Some(summary) = &mut table_ledger {
            if let Some(ledger) = test_ledger.as_ref() {
                summary.record(ledger);
            }
            if let Some(ledger) = ref_ledger.as_ref() {
                summary.record(ledger);
            }
        }

        let matches = images_match(&test_img, &ref_img, Some(fuzzy));
        let pass = match kind {
            MatchKind::Match => matches,
            MatchKind::Mismatch => !matches,
        };
        if pass {
            passed += 1;
            if let Some(reason) = reference_verification.reason_for(test.name()) {
                reference_unverified += 1;
                actuals.push(ActualRecord::with_reason(test, "pass", reason));
                if args.verbose {
                    println!("PASS  reference-unverified {}", test.name());
                }
            } else {
                actuals.push(ActualRecord::new(test, "pass"));
                if args.verbose {
                    println!("PASS  {}", test.name());
                }
            }
        } else {
            failed += 1;
            actuals.push(ActualRecord::new(test, "fail"));
            let k = if kind == MatchKind::Match {
                "match   "
            } else {
                "mismatch"
            };
            // Diagnose the diff shape (Lever 2 triage). `match` failures get a
            // bucket from the test-vs-ref pixel diff; `mismatch` failures are
            // "matched when it shouldn't", a different shape, tallied separately.
            if kind == MatchKind::Match {
                let s = diff_stats(&test_img, &ref_img);
                let label = diff_label(&s);
                *buckets.entry(label).or_insert(0) += 1;
                if args.verbose {
                    let pct = (s.differing * 100).checked_div(s.total).unwrap_or(0);
                    println!(
                        "FAIL  {k} [{label:5}] diff={pct}% maxδ={} {}",
                        s.max_channel_diff,
                        test.name()
                    );
                } else {
                    println!("FAIL  {k} [{label:5}] {}", test.name());
                }
            } else {
                *buckets.entry("mismatch-eq").or_insert(0) += 1;
                println!("FAIL  {k} {}", test.name());
            }
        }
    }

    panic::set_hook(prev);

    println!(
        "\nreftest: {} passed ({} verified, {} reference-unverified), {} failed, {} skipped, {} errored (of {} files)",
        passed,
        passed - reference_unverified,
        reference_unverified,
        failed,
        skipped,
        errored,
        tests.len()
    );
    if reference_unverified != 0 {
        println!(
            "  reference verification: {} ({})",
            reference_verification.reason, reference_verification.source
        );
    }
    if !buckets.is_empty() {
        let mut sorted: Vec<_> = buckets.iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(a.1));
        let legend = "dims=size differs | whole=>=50% pixels (layout/UA) | \
                      aa=small per-channel (anti-alias/tolerance) | \
                      local=localized large (feature/paint) | equal?=identical-yet-failed";
        println!(
            "fail buckets: {}",
            sorted
                .iter()
                .map(|(k, n)| format!("{k}={n}"))
                .collect::<Vec<_>>()
                .join("  ")
        );
        println!("  ({legend})");
    }
    if let (Some(path), Some(summary)) = (&args.write_table_ledger, &table_ledger)
        && let Err(error) = write_table_ledger(path, summary)
    {
        eprintln!("cannot write table ledger {}: {error}", path.display());
        std::process::exit(1);
    }
    // Optional expectations baseline (write or check). When a baseline is
    // checked, its `unexpected=0` verdict owns the exit code; the raw
    // failed/errored exit only applies to un-guarded runs.
    finish_expectations(args, "reftest", &actuals);
    if args.expectations.is_none() && (failed > 0 || errored > 0) {
        std::process::exit(1);
    }
}
