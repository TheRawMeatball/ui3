#![feature(min_type_alias_impl_trait)]
#![feature(hash_drain_filter)]
#![feature(generic_associated_types)]

use std::{
    any::{Any, TypeId},
    cell::RefCell,
    collections::HashMap,
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

pub struct Application<B: UiBackend> {
    root: MountedWidgetNode<B>,
}

impl<B: UiBackend> Application<B> {
    pub fn update(&mut self, ctx: &mut B::RunCtx) {
        let mut ctx = Context::<B> { backend_data: ctx };
        self.root.process(&mut ctx);
    }

    pub fn new(root: WidgetNode<B>, ctx: &mut B::RunCtx) -> Self {
        let mut ctx = Context::<B> { backend_data: ctx };
        Self {
            root: root.mount(&mut ctx),
        }
    }

    pub fn render(&self) -> RenderChildIterator<'_, B> {
        self.root.render()
    }
}

pub struct RenderNode<'a, B: UiBackend> {
    children: &'a MountedWidgetNodeGroup<B>,
    pub unit: &'a B::Unit,
}

impl<'a, B: UiBackend> RenderNode<'a, B> {
    pub fn iter_children(&self) -> GroupIterator<'a, B> {
        self.children.render()
    }
}

pub struct Context<'a, B: UiBackend> {
    pub backend_data: &'a mut B::RunCtx,
}

pub trait WidgetFunc<P: 'static, B: UiBackend, Params>: 'static {
    fn call(&self, ctx: &mut Context<B>, props: &P, init_data: &mut dyn Any) -> WidgetNode<B>;
    fn init(&self, ctx: &mut Context<B>) -> Box<dyn Any>;
    fn deinit(&self, ctx: &mut Context<B>, init_data: Box<dyn Any>);
    fn needs_recalc(&self, ctx: &Context<B>, init_data: &dyn Any) -> bool;
    fn as_dynamic(&self) -> Box<dyn DynWidgetFunc<B>>;
    fn fn_type_id(&self) -> TypeId;
    fn w(self, props: P) -> WidgetNode<B>
    where
        Self: Sized,
    {
        WidgetNode::Component(WidgetComponent {
            func: self.as_dynamic(),
            props: Box::new(props),
        })
    }
}
pub trait DynWidgetFunc<B: UiBackend>: 'static {
    fn call(&self, ctx: &mut Context<B>, props: &dyn Any, init_data: &mut dyn Any)
        -> WidgetNode<B>;
    fn needs_recalc(&self, ctx: &Context<B>, init_data: &dyn Any) -> bool;
    fn init(&self, ctx: &mut Context<B>) -> Box<dyn Any>;
    fn deinit(&self, ctx: &mut Context<B>, init_data: Box<dyn Any>);
    fn dyn_clone(&self) -> Box<dyn DynWidgetFunc<B>>;
    fn fn_type_id(&self) -> TypeId;
}

impl<P: 'static, B: UiBackend, Params: 'static> DynWidgetFunc<B>
    for Box<dyn WidgetFunc<P, B, Params>>
{
    fn call(
        &self,
        ctx: &mut Context<B>,
        props: &dyn Any,
        init_data: &mut dyn Any,
    ) -> WidgetNode<B> {
        (**self).call(ctx, props.downcast_ref().unwrap(), init_data)
    }

    fn needs_recalc(&self, ctx: &Context<B>, init_data: &dyn Any) -> bool {
        (**self).needs_recalc(ctx, init_data)
    }

    fn dyn_clone(&self) -> Box<dyn DynWidgetFunc<B>> {
        (**self).as_dynamic()
    }

    fn fn_type_id(&self) -> TypeId {
        (**self).fn_type_id()
    }

    fn init(&self, stores: &mut Context<B>) -> Box<dyn Any> {
        (**self).init(stores)
    }

    fn deinit(&self, ctx: &mut Context<B>, init_data: Box<dyn Any>) {
        (**self).deinit(ctx, init_data)
    }
}

pub struct WidgetComponent<B: UiBackend> {
    func: Box<dyn DynWidgetFunc<B>>,
    props: Box<dyn Any>,
}

impl<B: UiBackend> WidgetNode<B> {
    fn mount(self, ctx: &mut Context<B>) -> MountedWidgetNode<B> {
        match self {
            WidgetNode::None => MountedWidgetNode::None,
            WidgetNode::Component(c) => {
                let mw = c.mount(ctx);
                MountedWidgetNode::Component(mw)
            }
            WidgetNode::Unit { children, unit } => MountedWidgetNode::Unit {
                unit,
                children: children.mount(ctx),
            },
            WidgetNode::Group(group) => MountedWidgetNode::Group(group.mount(ctx)),
        }
    }
}

impl<B: UiBackend> WidgetComponent<B> {
    fn mount(self, ctx: &mut Context<B>) -> MountedWidgetComponent<B> {
        let mut init_data = self.func.init(ctx);

        let result = self
            .func
            .call(ctx, &*self.props, &mut *init_data)
            .mount(ctx);

        MountedWidgetComponent {
            template: self,
            init_data,
            result: Box::new(result),
        }
    }
}

pub enum WidgetNode<B: UiBackend> {
    None,
    Component(WidgetComponent<B>),
    Unit {
        unit: B::Unit,
        children: WidgetNodeGroup<B>,
    },
    Group(WidgetNodeGroup<B>),
}

pub struct WidgetNodeGroup<B: UiBackend> {
    render_order: Vec<IntOrString>,
    ordered: Vec<WidgetNode<B>>,
    named: HashMap<String, WidgetNode<B>>,
}

impl<B: UiBackend> Default for WidgetNodeGroup<B> {
    fn default() -> Self {
        Self {
            render_order: Default::default(),
            ordered: Default::default(),
            named: Default::default(),
        }
    }
}

impl<B: UiBackend> WidgetNodeGroup<B> {
    pub fn single(node: WidgetNode<B>) -> Self {
        let mut this = Self::default();
        this.push(node);
        this
    }
}

struct MountedWidgetNodeGroup<B: UiBackend> {
    render_order: Vec<IntOrString>,
    ordered: Vec<MountedWidgetNode<B>>,
    named: HashMap<String, MountedWidgetNode<B>>,
}

#[derive(Clone)]
pub enum IntOrString {
    Int(usize),
    String(String),
}

impl<B: UiBackend> MountedWidgetNodeGroup<B> {
    fn unmount(self, ctx: &mut Context<B>) {
        self.named
            .into_iter()
            .map(|(_, v)| v)
            .chain(self.ordered)
            .for_each(|node| node.unmount(ctx));
    }

    fn diff(&mut self, new: WidgetNodeGroup<B>, ctx: &mut Context<B>) {
        let WidgetNodeGroup {
            named: mut new_named,
            ordered: new_ordered,
            render_order: new_render_order,
        } = new;
        self.render_order = new_render_order;
        if new_ordered.len() < self.ordered.len() {
            self.ordered
                .drain(new_ordered.len()..)
                .for_each(|w| w.unmount(ctx));
        }

        self.ordered
            .iter_mut()
            .zip(new_ordered)
            .for_each(|(old, new)| old.diff(new, ctx));

        let ctx = RefCell::new(ctx);
        self.named
            .drain_filter(|name, old| {
                new_named
                    .remove(name)
                    .map(|new| old.diff(new, *ctx.borrow_mut()))
                    .is_none()
            })
            .for_each(|(_, w)| w.unmount(*ctx.borrow_mut()));
    }

    fn process(&mut self, ctx: &mut Context<B>) {
        self.named
            .iter_mut()
            .map(|(_, v)| v)
            .chain(&mut self.ordered)
            .for_each(|node| node.process(ctx));
    }

    fn render(&self) -> GroupIterator<'_, B> {
        self.render_order.iter().flat_map(get_magic_func(self))
    }
}

fn get_magic_func<'a, B: UiBackend>(ng: &'a MountedWidgetNodeGroup<B>) -> MagicFunc<'a, B> {
    let MountedWidgetNodeGroup { ordered, named, .. } = ng;
    move |n| {
        Box::new(match n {
            IntOrString::Int(i) => ordered[*i].render(),
            IntOrString::String(s) => named[s].render(),
        }) as Box<dyn Iterator<Item = RenderNode<'_, B>> + '_>
    }
}

impl<B: UiBackend> WidgetNodeGroup<B> {
    pub fn push(&mut self, node: WidgetNode<B>) {
        self.render_order.push(IntOrString::Int(self.ordered.len()));
        self.ordered.push(node);
    }
    pub fn push_named(&mut self, node: WidgetNode<B>, name: impl Into<String>) {
        let name = name.into();
        self.named
            .insert(name.clone(), node)
            .is_some()
            .then(|| panic!("Same key used multiple times!"));
        self.render_order.push(IntOrString::String(name));
    }

    fn mount(self, ctx: &mut Context<B>) -> MountedWidgetNodeGroup<B> {
        let Self {
            named,
            ordered,
            render_order,
        } = self;
        MountedWidgetNodeGroup {
            named: named.into_iter().map(|(n, w)| (n, w.mount(ctx))).collect(),
            ordered: ordered.into_iter().map(|w| w.mount(ctx)).collect(),
            render_order,
        }
    }
}

enum MountedWidgetNode<B: UiBackend> {
    None,
    Component(MountedWidgetComponent<B>),
    Unit {
        unit: B::Unit,
        children: MountedWidgetNodeGroup<B>,
    },
    Group(MountedWidgetNodeGroup<B>),
}

impl<B: UiBackend> MountedWidgetNode<B> {
    fn diff(&mut self, new: WidgetNode<B>, ctx: &mut Context<B>) {
        match (self, new) {
            (MountedWidgetNode::None, WidgetNode::None) => {}
            (MountedWidgetNode::Component(c), WidgetNode::Component(new)) => c.diff(new, ctx),
            (
                MountedWidgetNode::Unit { unit, children },
                WidgetNode::Unit {
                    unit: new_unit,
                    children: new_children,
                },
            ) => {
                *unit = new_unit;
                children.diff(new_children, ctx);
            }
            (MountedWidgetNode::Group(old), WidgetNode::Group(new)) => old.diff(new, ctx),
            (this, new) => std::mem::replace(this, new.mount(ctx)).unmount(ctx),
        }
    }

    fn unmount(self, ctx: &mut Context<B>) {
        match self {
            MountedWidgetNode::None => {}
            MountedWidgetNode::Component(c) => c.unmount(ctx),
            MountedWidgetNode::Unit { children, .. } => children.unmount(ctx),
            MountedWidgetNode::Group(group) => group.unmount(ctx),
        }
    }

    fn process(&mut self, ctx: &mut Context<B>) {
        match self {
            MountedWidgetNode::None => {}
            MountedWidgetNode::Component(c) => c.process(ctx, false),
            MountedWidgetNode::Unit { children, .. } => children.process(ctx),
            MountedWidgetNode::Group(group) => group.process(ctx),
        }
    }

    fn render(&self) -> RenderChildIterator<'_, B> {
        match self {
            MountedWidgetNode::None => RenderChildIterator::Empty(std::iter::empty()),
            MountedWidgetNode::Component(c) => {
                return c.result.render();
            }
            MountedWidgetNode::Unit { unit, children } => {
                RenderChildIterator::Once(std::iter::once(RenderNode { children, unit }))
            }
            MountedWidgetNode::Group(g) => RenderChildIterator::Group(g.render()),
        }
    }
}

type GroupIterator<'a, B> = std::iter::FlatMap<
    std::slice::Iter<'a, IntOrString>,
    Box<dyn Iterator<Item = RenderNode<'a, B>> + 'a>,
    MagicFunc<'a, B>,
>;

type MagicFunc<'a, B> = impl Fn(&IntOrString) -> Box<dyn Iterator<Item = RenderNode<'a, B>> + 'a>;

pub enum RenderChildIterator<'a, B: UiBackend> {
    Empty(std::iter::Empty<RenderNode<'a, B>>),
    Once(std::iter::Once<RenderNode<'a, B>>),
    Group(GroupIterator<'a, B>),
}

impl<'a, B: UiBackend> Iterator for RenderChildIterator<'a, B> {
    type Item = RenderNode<'a, B>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            RenderChildIterator::Empty(i) => i.next(),
            RenderChildIterator::Once(i) => i.next(),
            RenderChildIterator::Group(i) => i.next(),
        }
    }
}

struct MountedWidgetComponent<B: UiBackend> {
    template: WidgetComponent<B>,
    result: Box<MountedWidgetNode<B>>,
    init_data: Box<dyn Any>,
}

impl<B: UiBackend> MountedWidgetComponent<B> {
    fn process(&mut self, ctx: &mut Context<B>, force_recalc: bool) {
        if !force_recalc && !self.template.func.needs_recalc(ctx, &*self.init_data) {
            self.result.process(ctx);
            return;
        }

        let new_result = self
            .template
            .func
            .call(ctx, &*self.template.props, &mut *self.init_data);

        self.result.diff(new_result, ctx);
    }

    fn diff(&mut self, new: WidgetComponent<B>, ctx: &mut Context<B>) {
        if self.template.func.fn_type_id() == new.func.fn_type_id() {
            self.template.props = new.props;
            self.process(ctx, true);
        } else {
            std::mem::replace(self, new.mount(ctx)).unmount(ctx)
        }
    }

    fn unmount(self, ctx: &mut Context<B>) {
        self.result.unmount(ctx);
        self.template.func.deinit(ctx, self.init_data);
    }
}

pub trait UiBackend: 'static {
    type Unit;
    type RunCtx;
}

pub trait WidgetParam<B: UiBackend>: 'static {
    type InitData: 'static;
    type Item<'a>;

    fn init(ctx: &mut Context<B>) -> Self::InitData;
    fn deinit(ctx: &mut Context<B>, init_data: Self::InitData);
    fn get_item<'a>(ctx: &'a Context<B>, init_data: &mut Self::InitData) -> Self::Item<'a>;
    fn needs_recalc(ctx: &Context<B>, init_data: &Self::InitData) -> bool;
}

macro_rules! impl_functions {
    ($($idents: ident),*) => {
        impl_functions!([], [$($idents),*]);
    };
    ([$($head: ident),*], []) => {
        impl_functions!(@finalize [$($head),*], []);
    };
    ([$($head: ident),*], [$last: ident]) => {
        impl_functions!(@finalize [$($head),*], [$last]);
        impl_functions!([$($head,)* $last], []);
    };
    ([$($head: ident),*], [$transfer: ident, $($tail: ident),*]) => {
        impl_functions!(@finalize [$($head),*], [$($tail,)* $transfer]);
        impl_functions!([$($head,)* $transfer], [$($tail),*]);
    };
    (@finalize [$($props: ident),*], [$($params: ident),*]) => {
        #[allow(unused)]
        #[allow(non_snake_case)]
        impl<Backend: UiBackend, Func, $($props,)* $($params,)*> WidgetFunc<($($props,)*), Backend, ($($params,)*)> for Func
        where
            Func: Fn($(&$props,)* $($params,)*) -> WidgetNode<Backend> + Copy + 'static,
            Func: for<'a> Fn($(&$props,)* $(<$params as WidgetParam<Backend>>::Item<'a>,)*) -> WidgetNode<Backend> + Copy + 'static,
            $($props: 'static,)*
            $($params: WidgetParam<Backend>,)*
        {
            fn call(&self, ctx: &mut Context<Backend>, ($($props,)*): &($($props,)*), init_data: &mut dyn Any) -> WidgetNode<Backend> {
                let ($($params,)*): &mut ($(<$params as WidgetParam<Backend>>::InitData,)*) = init_data.downcast_mut().unwrap();
                (self)($($props,)* $($params::get_item(&ctx, $params),)*)
            }

            fn needs_recalc(&self, ctx: &Context<Backend>, init_data: &dyn Any) -> bool {
                let ($($params,)*): &($(<$params as WidgetParam<Backend>>::InitData,)*) = init_data.downcast_ref().unwrap();
                false $(|| $params::needs_recalc(ctx, $params))*
            }

            fn init(&self, ctx: &mut Context<Backend>) -> Box<dyn Any> {
                Box::new(($($params::init(ctx),)*))
            }

            fn deinit(&self, ctx: &mut Context<Backend>, init_data: Box<dyn Any>) {
                let ($($params,)*): ($(<$params as WidgetParam<Backend>>::InitData,)*) = *init_data.downcast().unwrap();
                $($params::deinit(ctx, $params);)*
            }

            fn as_dynamic(&self) -> Box<dyn DynWidgetFunc<Backend>> {
                Box::new(Box::new(*self) as Box<dyn WidgetFunc<($($props,)*), Backend, ($($params,)*)>>)
            }

            fn fn_type_id(&self) -> TypeId {
                TypeId::of::<Func>()
            }
        }
    };
}

impl_functions!();
impl_functions!(A);
impl_functions!(A, B);
impl_functions!(A, B, C);
impl_functions!(A, B, C, D);
impl_functions!(A, B, C, D, E);
impl_functions!(A, B, C, D, E, F);
impl_functions!(A, B, C, D, E, F, G);
impl_functions!(A, B, C, D, E, F, G, H);
impl_functions!(A, B, C, D, E, F, G, H, I);
impl_functions!(A, B, C, D, E, F, G, H, I, J);
impl_functions!(A, B, C, D, E, F, G, H, I, J, K);
impl_functions!(A, B, C, D, E, F, G, H, I, J, K, L);
