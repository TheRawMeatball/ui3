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
    console_log!("running app, store: {}", *store);
    let mut group = WidgetNodeGroup::default();
    let id = store.id();
    group.push(textw.w((format!("hello! number: {}", *store),)));
    group.push(buttonw.w((
        Rc::new(move |ctx| {
            console_log!("increment button pressed!");
            *id.access(ctx) += 1;
        }),
        Rc::new(|| {
            let mut group = WidgetNodeGroup::default();
            group.push(textw.w(("Increment".into(),)));
            group
        }),
    )));
    group.push(buttonw.w((
        Rc::new(move |ctx| {
            console_log!("decrement button pressed!");
            *id.access(ctx) -= 1;
        }),
        Rc::new(|| {
            let mut group = WidgetNodeGroup::default();
            group.push(textw.w(("Decrement".into(),)));
            group
        }),
    )));
    Wn::Group(group)
}
