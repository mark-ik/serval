/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! A settings row over the `genet-host-api` settings projection vocabulary.
//!
//! [`setting_row`] renders one [`SettingSpec`] as a labelled control with an
//! Apply button. The in-progress edit (the draft) is component-local: it is
//! not application truth and never appears in the caller's state. The only
//! value that crosses the boundary is the applied [`SettingValue`], which the
//! caller lowers into its own action vocabulary and provider.
//!
//! The committed value remains parent-controlled: when a spec arrives with a
//! different `value` or `control` (an external write, a provider reload), the
//! draft re-derives from it. Editing the draft alone never resets it, because
//! the row is memoized on its props and a local edit marks the component
//! dirty rather than changing the spec.

use genet_host_api::settings::{SettingControl, SettingOption, SettingSpec, SettingValue};
use meristem::{MessageResult, View, lens, map_message_result};

use crate::component::{ComponentView, component};
use crate::{
    Action, GenetCtx, GenetElement, PointerClick, RadioGroup, Slider, TextInput, button, el,
    radio_group, slider, text_field_typed, toggle,
};

impl Action for SettingValue {}

/// Parent-owned props for one row: the spec plus the label text to render.
///
/// The label is separate from `spec.label` so a host can render a diagnostic
/// or annotated label without that presentation choice entering the spec.
#[derive(Clone, Debug, PartialEq)]
struct RowProps {
    spec: SettingSpec,
    label: String,
}

/// Component-local draft state for one row. Never the caller's concern.
enum SettingDraft {
    Text(TextInput),
    Number(Slider),
    Toggle(bool),
    Choice(RadioGroup),
    Unsupported,
}

fn number_range(min: Option<f64>, max: Option<f64>) -> (f64, f64) {
    let min = min.unwrap_or(0.0);
    let max = max
        .filter(|max| max.is_finite() && *max > min)
        .unwrap_or(min + 1.0);
    (min, max)
}

fn number_fraction(value: f64, min: Option<f64>, max: Option<f64>) -> f32 {
    let (min, max) = number_range(min, max);
    ((value - min) / (max - min)).clamp(0.0, 1.0) as f32
}

fn number_value(fraction: f32, min: Option<f64>, max: Option<f64>) -> f64 {
    let (min, max) = number_range(min, max);
    min + f64::from(fraction.clamp(0.0, 1.0)) * (max - min)
}

fn draft_for(spec: &SettingSpec) -> SettingDraft {
    match (&spec.control, &spec.value) {
        (SettingControl::Text, SettingValue::Text(value)) => {
            SettingDraft::Text(TextInput::new(value.clone()))
        },
        (SettingControl::Number { min, max, step }, SettingValue::Number(value)) => {
            let (low, high) = number_range(*min, *max);
            let range = high - low;
            let step_fraction = step.map(|step| (step / range) as f32).unwrap_or(0.01);
            SettingDraft::Number(
                Slider::new(number_fraction(*value, *min, *max))
                    .with_label(spec.label.clone())
                    .with_steps(step_fraction, (step_fraction * 5.0).max(0.1)),
            )
        },
        (SettingControl::Toggle, SettingValue::Boolean(checked)) => SettingDraft::Toggle(*checked),
        (SettingControl::Choice { options }, SettingValue::Text(value)) => {
            let selected = options
                .iter()
                .position(|option| option.value == *value)
                .unwrap_or(0);
            SettingDraft::Choice(RadioGroup::new(selected).with_label(spec.label.clone()))
        },
        _ => SettingDraft::Unsupported,
    }
}

/// The draft controls are `Action = ()` views that mutate the draft in place;
/// the Apply button is the row's only emitter. This adapter carries the
/// silent cluster into the row's `SettingValue`-typed tree.
fn silence(
    _: &mut SettingDraft,
    result: MessageResult<()>,
) -> MessageResult<SettingValue> {
    match result {
        MessageResult::Action(()) | MessageResult::Nop => MessageResult::Nop,
        MessageResult::RequestRebuild => MessageResult::RequestRebuild,
        MessageResult::Stale => MessageResult::Stale,
    }
}

fn apply_button<R>(
    setting_id: &str,
    read: R,
) -> impl View<SettingDraft, SettingValue, GenetCtx, Element = GenetElement> + use<R>
where
    R: Fn(&SettingDraft) -> SettingValue + 'static,
{
    button("Apply", move |draft: &mut SettingDraft, _: PointerClick| {
        read(draft)
    })
    .attr("class", "setting-apply")
    .attr("data-setting", setting_id.to_owned())
}

fn editor_row(
    props: &RowProps,
    control: impl View<SettingDraft, (), GenetCtx, Element = GenetElement> + 'static,
    read: impl Fn(&SettingDraft) -> SettingValue + 'static,
) -> ComponentView<SettingDraft, SettingValue> {
    let label = el::<_, SettingDraft, ()>("div", props.label.clone()).attr("class", "setting-label");
    let silenced = map_message_result(
        el::<_, SettingDraft, ()>("div", (label, control)).attr("class", "setting-editor"),
        silence,
    );
    Box::new(
        el::<_, SettingDraft, SettingValue>(
            "div",
            (silenced, apply_button(&props.spec.id, read)),
        )
        .attr("class", "setting-row")
        .attr("data-setting", props.spec.id.clone()),
    )
}

fn row_body(props: &RowProps, draft: &SettingDraft) -> ComponentView<SettingDraft, SettingValue> {
    match draft {
        SettingDraft::Text(_) => editor_row(
            props,
            lens(
                |input: &mut TextInput| text_field_typed(input),
                |draft: &mut SettingDraft| {
                    let SettingDraft::Text(input) = draft else {
                        unreachable!("text draft routes to a text editor");
                    };
                    input
                },
            ),
            |draft| {
                let SettingDraft::Text(input) = draft else {
                    unreachable!("text draft routes to a text apply");
                };
                SettingValue::Text(input.text().to_owned())
            },
        ),
        SettingDraft::Number(_) => {
            let (min, max) = match &props.spec.control {
                SettingControl::Number { min, max, .. } => (*min, *max),
                _ => (None, None),
            };
            editor_row(
                props,
                lens(
                    |control: &mut Slider| slider(control),
                    |draft: &mut SettingDraft| {
                        let SettingDraft::Number(control) = draft else {
                            unreachable!("number draft routes to a slider");
                        };
                        control
                    },
                ),
                move |draft| {
                    let SettingDraft::Number(control) = draft else {
                        unreachable!("number draft routes to a number apply");
                    };
                    SettingValue::Number(number_value(control.value, min, max))
                },
            )
        },
        SettingDraft::Toggle(_) => editor_row(
            props,
            lens(
                |checked: &mut bool| toggle(*checked),
                |draft: &mut SettingDraft| {
                    let SettingDraft::Toggle(checked) = draft else {
                        unreachable!("toggle draft routes to a toggle");
                    };
                    checked
                },
            ),
            |draft| {
                let SettingDraft::Toggle(checked) = draft else {
                    unreachable!("toggle draft routes to a toggle apply");
                };
                SettingValue::Boolean(*checked)
            },
        ),
        SettingDraft::Choice(_) => {
            let options: Vec<SettingOption> = match &props.spec.control {
                SettingControl::Choice { options } => options.clone(),
                _ => Vec::new(),
            };
            let display_options = options.clone();
            editor_row(
                props,
                lens(
                    move |choice: &mut RadioGroup| {
                        // Owned labels: `radio_group` builds retained rows from
                        // these values, so the view borrows nothing from here.
                        let labels: Vec<String> = display_options
                            .iter()
                            .map(|option| option.label.clone())
                            .collect();
                        radio_group(choice, &labels)
                    },
                    |draft: &mut SettingDraft| {
                        let SettingDraft::Choice(choice) = draft else {
                            unreachable!("choice draft routes to a radio group");
                        };
                        choice
                    },
                ),
                move |draft| {
                    let SettingDraft::Choice(choice) = draft else {
                        unreachable!("choice draft routes to a choice apply");
                    };
                    let value = options
                        .get(choice.selected)
                        .or_else(|| options.first())
                        .map(|option| option.value.clone())
                        .unwrap_or_default();
                    SettingValue::Text(value)
                },
            )
        },
        SettingDraft::Unsupported => Box::new(
            el::<_, SettingDraft, SettingValue>("div", "Unsupported control/value pair")
                .attr("class", "setting-row setting-unsupported")
                .attr("data-setting", props.spec.id.clone()),
        ),
    }
}

/// One settings row: label, draft-local control, Apply.
///
/// `on_apply` receives the applied [`SettingValue`] and lowers it into the
/// caller's action vocabulary; the caller already knows the setting id it
/// passed in. The row is memoized on `(spec, label)`, so unchanged rows skip
/// their body on parent rebuilds.
pub fn setting_row<State, A, L, F>(
    spec: &SettingSpec,
    label: L,
    on_apply: F,
) -> impl View<State, A, GenetCtx, Element = GenetElement> + use<State, A, L, F>
where
    State: 'static,
    A: 'static,
    L: Into<String>,
    F: Fn(&mut State, SettingValue) -> A + 'static,
{
    component(
        RowProps {
            spec: spec.clone(),
            label: label.into(),
        },
        |props: &RowProps| draft_for(&props.spec),
        |prev: &RowProps, next: &RowProps, draft: &mut SettingDraft| {
            if prev.spec.value != next.spec.value || prev.spec.control != next.spec.control {
                *draft = draft_for(&next.spec);
            }
        },
        row_body,
        on_apply,
    )
    .memo()
}
