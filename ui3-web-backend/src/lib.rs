#![feature(generic_associated_types)]

use std::{borrow::Cow, cell::RefCell, collections::HashMap, marker::PhantomData, rc::Rc};

use ui3_core::{
    Application, Context, RenderNode, UiBackend, WidgetNode, WidgetNodeGroup, WidgetParam,
};
use virtual_dom_rs::{
    Closure, DomUpdater, DynClosure, Event, Events, HtmlElement, VElement, VText, VirtualNode,
};

use bevy_ecs::{
    prelude::{Entity, Mut},
    world::World,
};

use wasm_bindgen::{prelude::wasm_bindgen, JsValue};

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

pub struct WebBackend {}

impl UiBackend for WebBackend {
    type Unit = Unit;
    type RunCtx = World;
}

#[derive(Clone)]
pub enum Unit {
    Element {
        tag: &'static str,
        attrs: HashMap<&'static str, Cow<'static, str>>,
        events: HashMap<&'static str, DynClosure>,
    },
    Text(String),
}

pub type Ctx<'a> = Context<'a, WebBackend>;
pub type Wn = WidgetNode<WebBackend>;
pub type Wng = WidgetNodeGroup<WebBackend>;
pub type UiApp = Application<WebBackend>;

pub fn htmlw(
    tag: &&'static str,
    attrs: &HashMap<&'static str, Cow<'static, str>>,
    events: &HashMap<&'static str, DynClosure>,
    children: &Rc<Wn>,
) -> Wn {
    build_html(&tag, attrs.clone(), events.clone(), children.clone())
}

fn build_html(
    tag: &'static str,
    attrs: HashMap<&'static str, Cow<'static, str>>,
    events: HashMap<&'static str, DynClosure>,
    children: Rc<Wn>,
) -> Wn {
    Wn::Unit {
        unit: Unit::Element { tag, attrs, events },
        children,
    }
}

pub struct Store<'a, T> {
    val: &'a T,
    id: Entity,
}

impl<'a, T> Store<'a, T> {
    pub fn id(&self) -> StoreId<T> {
        StoreId {
            id: self.id,
            _m: PhantomData,
        }
    }
}

impl<'a, T> std::ops::Deref for Store<'a, T> {
    type Target = &'a T;

    fn deref(&self) -> &Self::Target {
        &self.val
    }
}

pub struct StoreId<T> {
    pub id: Entity,
    _m: PhantomData<T>,
}

impl<T: Send + Sync + 'static> StoreId<T> {
    pub fn access<'a>(&self, ctx: &'a mut Ctx) -> Mut<'a, T> {
        // Safety: safe because of repr transparent on wpwrapper.
        // I know I said I wouldn't, but this just felt simpler and I'm weak
        unsafe {
            std::mem::transmute::<Mut<'a, WPWrapper<T>>, Mut<'a, T>>(
                ctx.backend_data.get_mut(self.id).unwrap(),
            )
        }
    }
}

impl<T> Clone for StoreId<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            _m: PhantomData,
        }
    }
}

impl<T> Copy for StoreId<T> {}

#[repr(transparent)]
struct WPWrapper<T>(T);

impl<T: Default + Send + Sync + 'static> WidgetParam<WebBackend> for Store<'static, T> {
    type InitData = (Entity, u32);

    type Item<'a> = Store<'a, T>;

    fn init(ctx: &mut Context<WebBackend>) -> Self::InitData {
        let mut prop_entity = ctx.backend_data.spawn();
        prop_entity.insert(WPWrapper(T::default()));
        (prop_entity.id(), 0)
    }

    fn deinit(ctx: &mut Context<WebBackend>, init_data: Self::InitData) {
        ctx.backend_data.entity_mut(init_data.0).despawn();
    }

    fn get_item<'a>(
        ctx: &'a Context<WebBackend>,
        init_data: &mut Self::InitData,
    ) -> Self::Item<'a> {
        let id = init_data.0;
        init_data.1 = ctx.backend_data.read_change_tick();
        Store {
            val: &ctx.backend_data.get::<WPWrapper<T>>(id).unwrap().0,
            id,
        }
    }

    fn needs_recalc(ctx: &Context<WebBackend>, init_data: &Self::InitData) -> bool {
        ctx.backend_data
            .get_entity(init_data.0)
            .unwrap()
            .get_change_ticks::<WPWrapper<T>>()
            .unwrap()
            .is_changed(init_data.1, ctx.backend_data.read_change_tick())
    }
}

pub fn textw(text: &String) -> Wn {
    Wn::Unit {
        unit: Unit::Text(text.clone()),
        children: Rc::new(Wn::None),
    }
}

pub fn buttonw(f: &Rc<dyn Fn(&mut Ctx) + 'static>, children: &Rc<Wn>) -> Wn {
    let f = f.clone();
    build_html(
        "button",
        Default::default(),
        {
            let mut map: HashMap<&'static str, DynClosure> = Default::default();
            let closure = Closure::wrap(Box::new(move || {
                access_ctx(&*f);
            }) as Box<dyn Fn()>);
            map.insert("onclick", Rc::new(closure));
            map
        },
        children.clone(),
    )
}

pub fn textboxw(store: &StoreId<String>) -> Wn {
    let store = store.clone();
    build_html(
        "input",
        {
            let mut map: HashMap<&'static str, Cow<'static, str>> = Default::default();
            map.insert("type", "text".into());
            map
        },
        {
            let mut map: HashMap<&'static str, DynClosure> = Default::default();
            let closure = Closure::wrap(Box::new(move |e: Event| {
                access_ctx(|ctx| {
                    let new_value: String =
                        web_sys::HtmlInputElement::from(JsValue::from(e.target().unwrap())).value();
                    *store.access(ctx) = new_value;
                })
            }) as Box<dyn Fn(Event)>);
            map.insert("oninput", Rc::new(closure));
            map
        },
        Rc::new(Wn::None),
    )
}

fn access_ctx(f: impl FnOnce(&mut Ctx)) {
    RUNTIME
        .try_with(|val| {
            let mut rt = val.borrow_mut();
            let runtime = rt.as_mut().unwrap();
            let mut ctx = Ctx {
                backend_data: &mut runtime.world,
            };
            f(&mut ctx);
            runtime.tick();
        })
        .unwrap();
}

struct Runtime {
    world: World,
    updater: DomUpdater,
    app: UiApp,
}

impl Runtime {
    fn tick(&mut self) {
        self.app.update(&mut self.world);
        self.updater.update(direct_render(&self.app));
        self.world.increment_change_tick();
    }
}

thread_local! {
    static RUNTIME: RefCell<Option<Runtime>> = RefCell::new(None);
}

pub fn enter_runtime(root: Wn) {
    fn get_body() -> Option<HtmlElement> {
        Some(virtual_dom_rs::window()?.document()?.body()?)
    }

    console_error_panic_hook::set_once();
    let mut world = World::default();
    let app = UiApp::new(root, &mut world);
    let element = get_body().unwrap();
    let root = direct_render(&app);

    let updater = virtual_dom_rs::DomUpdater::new_append_to_mount(root, &element);
    world.increment_change_tick();

    RUNTIME
        .try_with(|val| {
            *val.borrow_mut() = Some(Runtime {
                world,
                updater,
                app,
            })
        })
        .unwrap();
}

fn direct_render(app: &UiApp) -> VirtualNode {
    fn extender(node: RenderNode<WebBackend>) -> VirtualNode {
        let mut cloned = VirtualNode::from(node.unit);
        if let VirtualNode::Element(e) = &mut cloned {
            e.children.extend(node.iter_children().map(extender));
        }
        cloned
    }
    let mut root = VElement::new("div");
    root.children.extend(app.render().map(extender));
    VirtualNode::Element(root)
}

impl From<&Unit> for VirtualNode {
    fn from(u: &Unit) -> Self {
        match u {
            Unit::Element { tag, attrs, events } => VirtualNode::Element(VElement {
                tag: (*tag).to_owned(),
                attrs: attrs
                    .iter()
                    .map(|(k, v)| ((*k).to_owned(), v.clone().into_owned()))
                    .collect(),
                events: Events(
                    events
                        .iter()
                        .map(|(k, v)| ((*k).to_owned(), v.clone()))
                        .collect(),
                ),
                children: vec![],
            }),
            Unit::Text(text) => VirtualNode::Text(VText { text: text.clone() }),
        }
    }
}
