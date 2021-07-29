#![feature(generic_associated_types)]

use std::{borrow::Cow, cell::RefCell, collections::HashMap, marker::PhantomData, rc::Rc};

use ui3_core::{
    Application, Context, UiBackend, WidgetContext, WidgetNode, WidgetNodeGroup, WidgetParam,
};

use bevy_ecs::{
    prelude::{Entity, Mut},
    world::World,
};

use wasm_bindgen::prelude::*;
use web_sys::{Event, HtmlElement};

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
    type RunCtx<'a> = RunCtx<'a>;
    type WidgetCtx<'a> = WidgetCtx<'a>;

    fn make_wctx<'a>(ctx: &'a Context<Self>) -> Self::WidgetCtx<'a> {
        WidgetCtx {
            world: ctx.backend_data.world,
        }
    }

    fn diff_units(
        old: &mut Self::Unit,
        new: &Self::Unit,
        ctx: &mut Context<Self>,
        diff_children: impl FnOnce(&mut Context<Self>),
    ) {
        *old = new.clone();
        diff_children(ctx);
    }

    fn mark_update(ctx: &mut Self::RunCtx<'_>) {
        ctx.world.increment_change_tick();
    }
}

pub struct RunCtx<'a> {
    world: &'a mut World,
}

pub struct WidgetCtx<'a> {
    world: &'a World,
}

type DynClosure = Rc<dyn AsRef<JsValue>>;

#[derive(Clone)]
pub enum Unit {
    Element {
        tag: &'static str,
        attrs: HashMap<&'static str, Cow<'static, str>>,
        events: HashMap<&'static str, DynClosure>,
    },
    Text(String),
}

pub type Ctx<'a, 'ctx> = Context<'a, 'ctx, WebBackend>;
pub type Wctx<'a, 'ctx> = WidgetContext<'a, 'ctx, WebBackend>;
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
    pub fn access_mut<'a>(self, ctx: &'a mut Ctx) -> Mut<'a, T> {
        // Safety: safe because of repr transparent on wpwrapper.
        // I know I said I wouldn't, but this just felt simpler and I'm weak
        unsafe {
            std::mem::transmute::<Mut<'a, WPWrapper<T>>, Mut<'a, T>>(
                ctx.backend_data.world.get_mut(self.id).unwrap(),
            )
        }
    }

    pub fn access_w<'a>(self, wctx: &mut Wctx<'a, '_>) -> &'a T {
        let last_change_tick = wctx.backend_data.world.read_change_tick();
        wctx.add_dynamic_dep(Box::new(move |ctx: &Ctx| {
            ctx.backend_data
                .world
                .entity(self.id)
                .get_change_ticks::<WPWrapper<T>>()
                .unwrap()
                .is_changed(last_change_tick, ctx.backend_data.world.read_change_tick())
        }));
        &wctx
            .backend_data
            .world
            .get::<WPWrapper<T>>(self.id)
            .unwrap()
            .0
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
        let mut prop_entity = ctx.backend_data.world.spawn();
        prop_entity.insert(WPWrapper(T::default()));
        (prop_entity.id(), 0)
    }

    fn deinit(ctx: &mut Context<WebBackend>, init_data: Self::InitData) {
        ctx.backend_data.world.entity_mut(init_data.0).despawn();
    }

    fn get_item<'a>(
        ctx: &'a Context<WebBackend>,
        init_data: &mut Self::InitData,
    ) -> Self::Item<'a> {
        let id = init_data.0;
        init_data.1 = ctx.backend_data.world.read_change_tick();
        Store {
            val: &ctx.backend_data.world.get::<WPWrapper<T>>(id).unwrap().0,
            id,
        }
    }

    fn needs_recalc(ctx: &RunCtx, init_data: &Self::InitData) -> bool {
        ctx.world
            .get_entity(init_data.0)
            .unwrap()
            .get_change_ticks::<WPWrapper<T>>()
            .unwrap()
            .is_changed(init_data.1, ctx.world.read_change_tick())
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

pub fn textboxw(mut wctx: Wctx, store: &StoreId<String>) -> Wn {
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
                    *store.access_mut(ctx) = new_value;
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
            let mut rctx = RunCtx {
                world: &mut runtime.world,
            };
            let mut ctx = runtime.app.get_ctx(&mut rctx);
            f(&mut ctx);
            runtime.tick();
        })
        .unwrap();
}

struct Runtime {
    world: World,
    app: UiApp,
}

impl Runtime {
    fn tick(&mut self) {
        self.app.update(&mut RunCtx {
            world: &mut self.world,
        });
    }
}

thread_local! {
    static RUNTIME: RefCell<Option<Runtime>> = RefCell::new(None);
}

pub fn enter_runtime(root: Wn) {
    fn get_body() -> Option<HtmlElement> {
        Some(web_sys::window()?.document()?.body()?)
    }

    console_error_panic_hook::set_once();
    let mut world = World::default();
    let app = UiApp::new(root, &mut RunCtx { world: &mut world });
    let element = get_body().unwrap();

    RUNTIME
        .try_with(|val| *val.borrow_mut() = Some(Runtime { world, app }))
        .unwrap();
}
