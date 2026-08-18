//! Custom properties (harvest H1): `--name` declarations, `var()`
//! substitution with fallbacks, and cycle-scoped invalidation.
//!
//! The resolution shapes follow the fork's `custom_properties.rs` at rev
//! `b157d92526` (on-demand resolution with an explicit visiting stack,
//! cycle members poisoned individually, substitution size guard), reshaped
//! onto livery's string token layer. Custom property names are
//! case-sensitive and stored with their `--` prefix.

use std::collections::{BTreeMap, BTreeSet};

/// The computed custom-property map of one element, inherited wholesale by
/// its children. Values are fully substituted token strings.
pub type CustomProperties = BTreeMap<String, String>;

/// A rectangle published by the host through CSS `env()` variables.
///
/// Coordinates are logical CSS pixels in the viewport's top-left coordinate
/// space. The first consumer is Window Controls Overlay's title-bar area, but
/// the rectangle itself stays neutral so the environment can grow without
/// teaching the substitution engine about window systems.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvironmentRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl EnvironmentRect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    fn is_valid(self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.width.is_finite()
            && self.height.is_finite()
            && self.width >= 0.0
            && self.height >= 0.0
    }
}

/// Host-owned CSS environment values.
///
/// An absent rectangle means the Window Controls Overlay environment names
/// are unknown, so an authored `env(name, fallback)` uses its fallback and an
/// unguarded reference is invalid at computed-value time. This is distinct
/// from a present zero-sized rectangle.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CssEnvironment {
    titlebar_area: Option<EnvironmentRect>,
}

impl CssEnvironment {
    pub fn titlebar_area(self) -> Option<EnvironmentRect> {
        self.titlebar_area
    }

    pub fn set_titlebar_area(&mut self, area: Option<EnvironmentRect>) {
        self.titlebar_area = area.filter(|area| area.is_valid());
    }

    pub fn with_titlebar_area(mut self, area: EnvironmentRect) -> Self {
        self.set_titlebar_area(Some(area));
        self
    }

    fn value(self, name: &str) -> Option<String> {
        let area = self.titlebar_area?;
        let value = match name {
            "titlebar-area-x" => area.x,
            "titlebar-area-y" => area.y,
            "titlebar-area-width" => area.width,
            "titlebar-area-height" => area.height,
            _ => return None,
        };
        Some(format!("{value}px"))
    }
}

/// One `--name: value` declaration as authored.
#[derive(Clone, Debug, PartialEq)]
pub struct CustomDeclaration {
    /// Full case-sensitive name, including the `--` prefix.
    pub name: String,
    pub value: CustomDeclaredValue,
    pub important: bool,
}

/// A custom property's declared value, including the CSS-wide keywords.
#[derive(Clone, Debug, PartialEq)]
pub enum CustomDeclaredValue {
    Value(String),
    /// `initial`: the guaranteed-invalid value (the name is absent).
    Initial,
    Inherit,
    /// Custom properties inherit, so `unset` behaves as `inherit`.
    Unset,
}

/// Substitution failed: an unresolvable `var()` without a usable fallback,
/// or a runaway expansion. The referencing declaration is invalid at
/// computed-value time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SubstitutionError;

/// The fork's guard against exponential `var()` expansion, sized down for
/// livery's lanes.
const SUBSTITUTION_BUDGET: usize = 65_536;
const REFERENCE_DEPTH_LIMIT: usize = 128;

/// True when `value` contains a `var(` function outside quotes, at any
/// nesting depth. The declaration must then defer parsing until
/// computed-value time.
pub fn contains_var(value: &str) -> bool {
    contains_function(value, b"var")
}

/// True when a value contains a CSS environment substitution outside quotes.
pub fn contains_env(value: &str) -> bool {
    contains_function(value, b"env")
}

/// True when parsing must wait for either custom-property or host-environment
/// substitution at computed-value time.
pub fn contains_substitution(value: &str) -> bool {
    contains_var(value) || contains_env(value)
}

fn contains_function(value: &str, function: &[u8; 3]) -> bool {
    let bytes = value.as_bytes();
    let mut quote: Option<u8> = None;
    let mut escaped = false;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(active) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active {
                quote = None;
            }
            index += 1;
            continue;
        }
        match byte {
            b'\'' | b'"' => quote = Some(byte),
            byte if byte.eq_ignore_ascii_case(&function[0]) => {
                let boundary = index == 0 || !is_ident_byte(bytes[index - 1]);
                if boundary && is_function_open(&bytes[index..], function) {
                    return true;
                }
            },
            _ => {},
        }
        index += 1;
    }
    false
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

fn is_function_open(bytes: &[u8], function: &[u8; 3]) -> bool {
    bytes.len() >= 4 && bytes[..3].eq_ignore_ascii_case(function) && bytes[3] == b'('
}

/// Substitute every `var()` reference in `raw` against a fully resolved
/// map. Fallbacks substitute recursively; an unresolvable reference with
/// no fallback fails the whole value.
pub fn substitute(raw: &str, map: &CustomProperties) -> Result<String, SubstitutionError> {
    substitute_with_environment(raw, map, &CssEnvironment::default())
}

/// Substitute both `var()` and `env()` references. Fallbacks may contain
/// either function and are resolved recursively under the same size budget.
pub fn substitute_with_environment(
    raw: &str,
    map: &CustomProperties,
    environment: &CssEnvironment,
) -> Result<String, SubstitutionError> {
    let mut budget = SUBSTITUTION_BUDGET;
    substitute_with(
        raw,
        &mut |name| map.get(name).cloned(),
        &mut |name| environment.value(name),
        &mut budget,
    )
}

fn substitute_with(
    raw: &str,
    resolve_var: &mut dyn FnMut(&str) -> Option<String>,
    resolve_env: &mut dyn FnMut(&str) -> Option<String>,
    budget: &mut usize,
) -> Result<String, SubstitutionError> {
    let bytes = raw.as_bytes();
    let mut out = String::with_capacity(raw.len());
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut index = 0;
    while index < raw.len() {
        let ch = raw[index..].chars().next().expect("in-bounds char");
        if let Some(active) = quote {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == active {
                quote = None;
            }
            index += ch.len_utf8();
            continue;
        }
        match ch {
            '\'' | '"' => {
                quote = Some(ch);
                out.push(ch);
                index += ch.len_utf8();
            },
            'v' | 'V' | 'e' | 'E' if index == 0 || !is_ident_byte(bytes[index - 1]) => {
                let kind = if is_function_open(&bytes[index..], b"var") {
                    Some(true)
                } else if is_function_open(&bytes[index..], b"env") {
                    Some(false)
                } else {
                    None
                };
                let Some(is_var) = kind else {
                    out.push(ch);
                    index += ch.len_utf8();
                    continue;
                };
                let open = index + 3;
                let close = matching_paren(raw, open).ok_or(SubstitutionError)?;
                let arguments = &raw[open + 1..close];
                let (name, fallback) = split_var_arguments(arguments);
                let name = name.trim();
                if (is_var && (!name.starts_with("--") || name.len() <= 2))
                    || (!is_var
                        && (name.is_empty()
                            || name.starts_with("--")
                            || !name.bytes().all(is_ident_byte)))
                {
                    return Err(SubstitutionError);
                }
                let replacement = match if is_var {
                    resolve_var(name)
                } else {
                    resolve_env(name)
                } {
                    Some(value) => value,
                    None => match fallback {
                        Some(fallback) => {
                            substitute_with(fallback, resolve_var, resolve_env, budget)?
                        },
                        None => return Err(SubstitutionError),
                    },
                };
                *budget = budget
                    .checked_sub(replacement.len())
                    .ok_or(SubstitutionError)?;
                out.push_str(replacement.trim());
                index = close + 1;
            },
            _ => {
                out.push(ch);
                index += ch.len_utf8();
            },
        }
    }
    Ok(out)
}

/// Find the `)` matching the `(` at byte offset `open`, quote-aware.
fn matching_paren(input: &str, open: usize) -> Option<usize> {
    let bytes = input.as_bytes();
    debug_assert_eq!(bytes[open], b'(');
    let mut depth = 0_u32;
    let mut quote: Option<u8> = None;
    let mut escaped = false;
    for (offset, byte) in bytes.iter().copied().enumerate().skip(open) {
        if let Some(active) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active {
                quote = None;
            }
            continue;
        }
        match byte {
            b'\'' | b'"' => quote = Some(byte),
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(offset);
                }
            },
            _ => {},
        }
    }
    None
}

/// Split `var()` arguments at the first top-level comma: `--name` vs the
/// fallback (which keeps any further commas).
fn split_var_arguments(arguments: &str) -> (&str, Option<&str>) {
    let bytes = arguments.as_bytes();
    let mut depth = 0_u32;
    let mut quote: Option<u8> = None;
    let mut escaped = false;
    for (index, byte) in bytes.iter().copied().enumerate() {
        if let Some(active) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active {
                quote = None;
            }
            continue;
        }
        match byte {
            b'\'' | b'"' => quote = Some(byte),
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                return (&arguments[..index], Some(&arguments[index + 1..]));
            },
            _ => {},
        }
    }
    (arguments, None)
}

/// Resolve one element's custom-property map: the parent map (inheritance)
/// updated by this element's winning declarations, with `var()` references
/// among them substituted on demand and cycles invalidated member-wise.
pub fn resolve_custom_map(
    parent: Option<&CustomProperties>,
    winners: impl IntoIterator<Item = (String, CustomDeclaredValue)>,
) -> CustomProperties {
    resolve_custom_map_with_environment(parent, winners, &CssEnvironment::default())
}

/// Resolve custom properties with host environment values available to every
/// token stream and fallback.
pub fn resolve_custom_map_with_environment(
    parent: Option<&CustomProperties>,
    winners: impl IntoIterator<Item = (String, CustomDeclaredValue)>,
    environment: &CssEnvironment,
) -> CustomProperties {
    let mut resolved: CustomProperties = parent.cloned().unwrap_or_default();
    let mut raws: BTreeMap<String, String> = BTreeMap::new();
    for (name, value) in winners {
        match value {
            CustomDeclaredValue::Value(raw) => {
                resolved.remove(&name);
                raws.insert(name, raw);
            },
            CustomDeclaredValue::Initial => {
                resolved.remove(&name);
                raws.remove(&name);
            },
            CustomDeclaredValue::Inherit | CustomDeclaredValue::Unset => {
                raws.remove(&name);
                match parent.and_then(|parent| parent.get(&name)) {
                    Some(value) => {
                        resolved.insert(name, value.clone());
                    },
                    None => {
                        resolved.remove(&name);
                    },
                }
            },
        }
    }

    let names: Vec<String> = raws.keys().cloned().collect();
    let mut state = ResolutionState {
        raws,
        resolved,
        invalid: BTreeSet::new(),
        visiting: Vec::new(),
        environment: *environment,
    };
    for name in names {
        resolve_name(&name, &mut state);
    }
    state.resolved
}

struct ResolutionState {
    raws: BTreeMap<String, String>,
    resolved: CustomProperties,
    invalid: BTreeSet<String>,
    visiting: Vec<String>,
    environment: CssEnvironment,
}

fn resolve_name(name: &str, state: &mut ResolutionState) -> Option<String> {
    if let Some(done) = state.resolved.get(name) {
        return Some(done.clone());
    }
    if state.invalid.contains(name) {
        return None;
    }
    if let Some(position) = state.visiting.iter().position(|active| active == name) {
        // A reference cycle. Exactly the members of the cycle become
        // invalid; earlier entries on the stack may still recover
        // through fallbacks.
        let (invalid, visiting) = (&mut state.invalid, &state.visiting);
        for member in &visiting[position..] {
            invalid.insert(member.clone());
        }
        return None;
    }
    let raw = state.raws.get(name).cloned()?;
    if state.visiting.len() >= REFERENCE_DEPTH_LIMIT {
        state.invalid.insert(name.to_owned());
        return None;
    }
    state.visiting.push(name.to_owned());
    let mut budget = SUBSTITUTION_BUDGET;
    let environment = state.environment;
    let outcome = substitute_with(
        &raw,
        &mut |reference| resolve_name(reference, state),
        &mut |name| environment.value(name),
        &mut budget,
    );
    state.visiting.pop();
    if state.invalid.contains(name) {
        return None;
    }
    match outcome {
        Ok(value) => {
            state.resolved.insert(name.to_owned(), value.clone());
            Some(value)
        },
        Err(SubstitutionError) => {
            state.invalid.insert(name.to_owned());
            None
        },
    }
}
