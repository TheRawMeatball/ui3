#![feature(generic_associated_types)]

use std::{cell::RefCell, collections::HashMap, marker::PhantomData, rc::Rc};

use ui3_core::{
    Application, Context, RenderNode, UiBackend, WidgetFunc, WidgetNode, WidgetNodeGroup,
    WidgetParam,
};
use virtual_dom_rs::{
    Closure, DomUpdater, DynClosure, Events, HtmlElement, VElement, VText, VirtualNode,
};

use bevy_ecs::{
    prelude::{Entity, Mut},
    world::World,
};

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

pub struct WebBackend {}

impl UiBackend for WebBackend {
    type Unit = VirtualNode;
    type RunCtx = World;
}

pub type Ctx<'a> = Context<'a, WebBackend>;
pub type Wn = WidgetNode<WebBackend>;
pub type Wng = WidgetNodeGroup<WebBackend>;
pub type UiApp = Application<WebBackend>;

pub fn htmlw(
    tag: &String,
    attrs: &HashMap<String, String>,
    events: &HashMap<String, DynClosure>,
    children: &Rc<dyn Fn() -> Wng>,
) -> Wn {
    Wn::Unit {
        unit: VirtualNode::Element(VElement {
            tag: tag.clone(),
            attrs: attrs.clone(),
            events: Events(events.clone()),
            children: vec![],
        }),
        children: (children)(),
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
        Store {
            val: &ctx.backend_data.get::<WPWrapper<T>>(id).unwrap().0,
            id,
        }
    }

    fn needs_recalc(ctx: &Context<WebBackend>, init_data: &Self::InitData) -> bool {
        true // todo!()
    }
}

pub fn textw(text: &String) -> Wn {
    Wn::Unit {
        unit: VirtualNode::Text(VText { text: text.clone() }),
        children: Default::default(),
    }
}

pub fn buttonw(f: &Rc<dyn Fn(&mut Ctx) + 'static>, children: &Rc<dyn Fn() -> Wng + 'static>) -> Wn {
    let f = f.clone();
    htmlw.w((
        "button".into(),
        Default::default(),
        {
            let mut map: HashMap<String, DynClosure> = Default::default();
            let closure = Closure::wrap(Box::new(move || {
                RUNTIME
                    .try_with(|val| {
                        let mut rt = val.borrow_mut();
                        let runtime = rt.as_mut().unwrap();
                        let mut ctx = Ctx {
                            backend_data: &mut runtime.world,
                        };
                        (f)(&mut ctx);
                        runtime.app.update(&mut runtime.world);
                        console_log!("applying to real dom!");
                        runtime.updater.update(direct_render(&runtime.app))
                    })
                    .unwrap();
            }) as Box<dyn Fn()>);
            map.insert("onclick".to_owned(), Rc::new(closure));
            map
        },
        children.clone(),
    ))
}

struct Runtime {
    world: World,
    updater: DomUpdater,
    app: UiApp,
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
        let mut cloned: VirtualNode = clone_unit(node.unit);
        if let VirtualNode::Element(e) = &mut cloned {
            e.children.extend(node.iter_children().map(extender));
        }
        cloned
    }
    let mut root = VElement::new("div");
    root.children.extend(app.render().map(extender));
    VirtualNode::Element(root)
}

pub fn clone_unit(this: &VirtualNode) -> VirtualNode {
    match this {
        VirtualNode::Element(e) => VirtualNode::Element(VElement {
            tag: e.tag.clone(),
            attrs: e.attrs.clone(),
            events: Events(e.events.0.clone()),
            children: e.children.iter().map(clone_unit).collect(),
        }),
        VirtualNode::Text(t) => VirtualNode::Text(VText {
            text: t.text.clone(),
        }),
    }
}