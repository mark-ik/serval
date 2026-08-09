//! Headed smoke: a counter over the extracted host. Proves the host API from
//! the consumer side — window, layout, paint, click dispatch, focus, redraw
//! after resize and DPI change — with no application machinery at all.
//!
//! Run with `cargo run -p cambium-genet-winit-host --example smoke`.

use cambium::{clickable, el, text, AnyView, GenetCtx, GenetElement};
use cambium_genet_winit_host::{run, AppCtx, HostHooks, HostOptions, Init};

struct Counter {
    clicks: usize,
}

type Child = Box<dyn AnyView<Counter, (), GenetCtx, GenetElement>>;

fn root(state: &Counter) -> Child {
    Box::new(el(
        "div",
        (
            el("div", text("cambium-genet-winit-host smoke")).attr("class", "title"),
            clickable(
                el("div", text(format!("clicked {} times", state.clicks)))
                    .attr("class", "button"),
                |state: &mut Counter, _| state.clicks += 1,
            ),
        ),
    )
    .attr("class", "frame"))
}

const SHEET: &str = "
.frame { padding: 24px; font-size: 16px; }
.title { margin-bottom: 12px; }
.button { padding: 8px 12px; background: #29486b; color: #f0ebdd; width: 200px; }
.button:hover { background: #3a5d85; }
";

fn main() {
    let options = HostOptions {
        title: "host smoke".into(),
        initial_logical_size: (420.0, 200.0),
        ..Default::default()
    };
    let hooks: HostHooks<Counter, fn(&Counter) -> Child, Child> = HostHooks {
        frame: Box::new(|_ctx: &mut AppCtx<'_, _, _, _>| false),
        after_dispatch: Box::new(|_ctx| {}),
        after_frame: Box::new(|_ctx| {}),
        focused_text: Box::new(|_runner| None),
        key_intercept: Box::new(|_runner, _event| false),
    };
    run(
        options,
        |_window| Init {
            state: Counter { clicks: 0 },
            logic: root as fn(&Counter) -> Child,
            sheet: SHEET.to_string(),
        },
        hooks,
    )
    .expect("event loop");
}
