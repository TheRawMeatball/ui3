use std::rc::Rc;

use ui3_core::{WidgetFunc, WidgetNodeGroup};
use ui3_web_backend::{buttonw, textboxw, textw, Store, Wn};

use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen]
extern "C" {
    // Use `js_namespace` here to bind `console.log(..)` instead of just
    // `log(..)`
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

#[allow(unused_macros)]
macro_rules! console_log {
    // Note that this is using the `log` function imported above during
    // `bare_bones`
    ($($t:tt)*) => (log(&format_args!($($t)*).to_string()))
}

fn main() {
    ui3_web_backend::enter_runtime(app.w(()))
}

fn app(store: Store<i32>, text: Store<String>) -> Wn {
    console_log!("updating app! text: {}", *text);
    let mut group = WidgetNodeGroup::default();
    let id = store.id();
    let text_id = text.id();
    group.push(textboxw.w((text.id(),)));
    group.push(textw.w((format!("hello! number: {}", *store),)));
    group.push(buttonw.w((
        Rc::new(move |ctx| *id.access_mut(ctx) += 1),
        Rc::new(textw.w(("Incecrement".into(),))),
    )));
    group.push(buttonw.w((
        Rc::new(move |ctx| *id.access_mut(ctx) -= 1),
        Rc::new(textw.w(("Decrement".into(),))),
    )));
    group.push(buttonw.w((
        Rc::new(move |ctx| text_id.access_mut(ctx).clear()),
        Rc::new(textw.w(("Clear text".into(),))),
    )));
    Wn::Group(group)
}
