use std::rc::Rc;

use bevy_ecs::prelude::World;
use ui3_core::{RenderNode, WidgetFunc, WidgetNodeGroup};
use ui3_web_backend::{buttonw, textw, Ctx, Store, UiApp, WebBackend, Wn};
use virtual_dom_rs::{HtmlElement, VElement, VText, VirtualNode};

use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen]
extern "C" {
    // Use `js_namespace` here to bind `console.log(..)` instead of just
    // `log(..)`
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

macro_rules! console_log {
    // Note that this is using the `log` function imported above during
    // `bare_bones`
    ($($t:tt)*) => (log(&format_args!($($t)*).to_string()))
}

fn main() {
    ui3_web_backend::enter_runtime(app.w(()))
}

fn app(store: Store<i32>) -> Wn {
    let mut group = WidgetNodeGroup::default();
    let id = store.id();
    group.push(textw.w((format!("hello! number: {}", *store),)));
    group.push(buttonw.w((
        Rc::new(move |ctx| *id.access(ctx) += 1),
        Rc::new(|| WidgetNodeGroup::single(textw.w(("Incecrement".into(),)))),
    )));
    group.push(buttonw.w((
        Rc::new(move |ctx| *id.access(ctx) -= 1),
        Rc::new(|| WidgetNodeGroup::single(textw.w(("Decrement".into(),)))),
    )));
    Wn::Group(group)
}
