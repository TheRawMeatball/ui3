#![feature(hash_drain_filter)]
#![feature(generic_associated_types)]

use std::{
    any::{Any, TypeId},
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    rc::Rc,
};

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

pub struct Application<B: UiBackend> {
    root: MountedWidgetNode<B>,
    effects: Effects<B>,
}

impl<B: UiBackend> Application<B> {
    pub fn update(&mut self, ctx: &mut B::RunCtx<'_>) {
        let mut pre = vec![];
        loop {
            let mut ctx = Context::<B> { backend_data: ctx };
            (..).any(|_| {
                self.effects
                    .effects
                    .values_mut()
                    .all(|e| !e.process(&mut ctx, false))
            });
            let mut ctx = InternalContext {
                backend_data: ctx.backend_data,
                effects: &mut self.effects,
                prop_recalc_effects: &mut pre,
            };
            self.root.process(&mut ctx);
            B::mark_update(ctx.backend_data);
            if pre.is_empty() {
                break;
            }
            for id in pre.drain(..) {
                ctx.effects.effects[&id].process(ctx, true);
            }
        }
    }

    pub fn new(root: WidgetNode<B>, ctx: &mut B::RunCtx<'_>) -> Self {
        let mut effects = Effects::<B>::default();
        let mut ctx = InternalContext::<B> {
            backend_data: ctx,
            effects: &mut effects,
            prop_recalc_effects: vec![],
        };
        let mut this = Self {
            root: root.mount(&mut ctx),
            effects,
        };
        B::mark_update(ctx.backend_data);
        this.update(ctx);
    }

    pub fn get_ctx<'b1, 'b2, 'c, 'ctx>(
        &'b1 mut self,
        ctx: &'b2 mut B::RunCtx<'ctx>,
    ) -> Context<'c, 'ctx, B>
    where
        'b1: 'c,
        'b2: 'c,
    {
        Context { backend_data: ctx }
    }
}

pub struct InternalContext<'b, 'ctx, B: UiBackend> {
    pub backend_data: &'b mut B::RunCtx<'ctx>,
    effects: &'b mut Effects<B>,
    prop_recalc_effects: &'b mut Vec<u32>,
}

/// ask for this in event handlers etc.
pub struct Context<'b, 'ctx, B: UiBackend> {
    pub backend_data: &'b mut B::RunCtx<'ctx>,
}

struct Effects<B: UiBackend> {
    counter: u32,
    effects: BTreeMap<u32, Effect<B>>,
}

impl<B: UiBackend> Default for Effects<B> {
    fn default() -> Self {
        Self {
            counter: 0,
            effects: Default::default(),
        }
    }
}

struct Effect<B: UiBackend> {
    primary: Box<dyn Fn(&mut Context<B>) -> Box<dyn FnOnce(&mut Context<B>)>>,
    cleanup: Option<Box<dyn FnOnce(&mut Context<B>)>>,
    needs_rerun_fn: Box<dyn Fn(&Context<B>) -> bool>,
}

impl<B: UiBackend> Effect<B> {
    fn process(&mut self, ctx: &mut Context<B>, force_process: bool) -> bool {
        let recalced = force_process || (self.needs_rerun_fn)(&ctx);
        if recalced {
            if let Some(cleanup) = self.cleanup.take() {
                cleanup(ctx);
            }

            self.cleanup = Some((self.primary)(ctx))
        }
        recalced
    }
}

/// widgets can access this <- read only (ish)
pub struct WidgetContext<'b, 'ctx, B: UiBackend> {
    pub backend_data: &'b mut B::WidgetCtx<'ctx>,
    effects: &'b mut Effects<B>,
    deplist: &'b mut DynDepList<B>,
    all_effect_ids: &'b mut Vec<u32>,
    propped_effect_ids: &'b mut Vec<u32>,
}

impl<'b, 'ctx, B: UiBackend> WidgetContext<'b, 'ctx, B> {
    pub fn add_effect<E, C, D>(&mut self, f: E, uses_props: bool, change_detection_fn: D)
    where
        E: Fn(&mut Context<B>) -> C + 'static,
        C: FnOnce(&mut Context<B>) + 'static,
        D: Fn(&Context<B>) -> bool + 'static,
    {
        let id = self.effects.counter;
        self.effects.counter += 1;

        self.all_effect_ids.push(id);
        if uses_props {
            self.propped_effect_ids.push(id);
        }

        self.effects.effects.insert(
            id,
            Effect {
                primary: Box::new(move |ctx| Box::new(f(ctx)) as Box<dyn FnOnce(&mut Context<B>)>),
                cleanup: None,
                needs_rerun_fn: Box::new(change_detection_fn),
            },
        );
    }

    pub fn add_dynamic_dep(&mut self, f: impl Fn(&Context<B>) -> bool + 'static) {
        self.deplist.push(Box::new(f));
    }
}

pub trait WidgetFunc<P: 'static, B: UiBackend, Marker>: 'static {
    fn call(
        &self,
        ctx: &mut InternalContext<B>,
        props: &P,
        init_data: &mut MountedWidgetData<B>,
    ) -> WidgetNode<B>;
    fn init(&self, ctx: &mut InternalContext<B>) -> Box<dyn Any>;
    fn deinit(&self, ctx: &mut InternalContext<B>, init_data: Box<dyn Any>);
    fn needs_recalc(&self, ctx: &InternalContext<B>, init_data: &dyn Any) -> bool;
    fn as_dynamic(&self) -> Rc<dyn DynWidgetFunc<B>>;
    fn fn_type_id(&self) -> TypeId;
    fn w(self, props: P) -> WidgetNode<B>
    where
        Self: Sized,
    {
        WidgetNode::Component(WidgetComponent {
            func: self.as_dynamic(),
            props: Rc::new(props),
        })
    }
}
pub trait DynWidgetFunc<B: UiBackend>: 'static {
    fn call(
        &self,
        ctx: &mut InternalContext<B>,
        props: &dyn Any,
        data: &mut MountedWidgetData<B>,
    ) -> WidgetNode<B>;
    fn needs_recalc(&self, ctx: &InternalContext<B>, init_data: &dyn Any) -> bool;
    fn init(&self, ctx: &mut InternalContext<B>) -> Box<dyn Any>;
    fn deinit(&self, ctx: &mut InternalContext<B>, init_data: Box<dyn Any>);
    fn fn_type_id(&self) -> TypeId;
}

impl<P: 'static, B: UiBackend, Params: 'static> DynWidgetFunc<B>
    for Box<dyn WidgetFunc<P, B, Params>>
{
    fn call(
        &self,
        ctx: &mut InternalContext<B>,
        props: &dyn Any,
        data: &mut MountedWidgetData<B>,
    ) -> WidgetNode<B> {
        (**self).call(ctx, props.downcast_ref().unwrap(), data)
    }

    fn needs_recalc(&self, ctx: &InternalContext<B>, init_data: &dyn Any) -> bool {
        (**self).needs_recalc(ctx, init_data)
    }

    fn fn_type_id(&self) -> TypeId {
        (**self).fn_type_id()
    }

    fn init(&self, stores: &mut InternalContext<B>) -> Box<dyn Any> {
        (**self).init(stores)
    }

    fn deinit(&self, ctx: &mut InternalContext<B>, init_data: Box<dyn Any>) {
        (**self).deinit(ctx, init_data)
    }
}

pub struct WidgetComponent<B: UiBackend> {
    func: Rc<dyn DynWidgetFunc<B>>,
    props: Rc<dyn Any>,
}

impl<B: UiBackend> Clone for WidgetComponent<B> {
    fn clone(&self) -> Self {
        Self {
            func: self.func.clone(),
            props: self.props.clone(),
        }
    }
}

impl<B: UiBackend> WidgetNode<B> {
    fn mount(&self, ctx: &mut InternalContext<B>) -> MountedWidgetNode<B> {
        match self {
            WidgetNode::None => MountedWidgetNode::None,
            WidgetNode::Component(c) => {
                let mw = c.mount(ctx);
                MountedWidgetNode::Component(mw)
            }
            WidgetNode::Unit { children, unit } => MountedWidgetNode::Unit {
                unit: unit.clone(),
                children: Box::new(children.mount(ctx)),
            },
            WidgetNode::Group(group) => MountedWidgetNode::Group(group.mount(ctx)),
        }
    }
}

impl<B: UiBackend> WidgetComponent<B> {
    fn mount(&self, ctx: &mut InternalContext<B>) -> MountedWidgetComponent<B> {
        let mut data = MountedWidgetData {
            deplist: vec![],
            all_effect_ids: vec![],
            propped_effect_ids: vec![],
            init_data: self.func.init(ctx),
        };
        let result = self.func.call(ctx, &*self.props, &mut data);
        let result = result.mount(ctx);

        MountedWidgetComponent {
            template: self.clone(),
            result: Box::new(result),
            data,
        }
    }
}

pub enum WidgetNode<B: UiBackend> {
    None,
    Component(WidgetComponent<B>),
    Unit {
        unit: B::Unit,
        children: Rc<WidgetNode<B>>,
    },
    Group(WidgetNodeGroup<B>),
}

impl<B: UiBackend> Clone for WidgetNode<B> {
    fn clone(&self) -> Self {
        match self {
            WidgetNode::None => WidgetNode::None,
            WidgetNode::Component(c) => WidgetNode::Component(c.clone()),
            WidgetNode::Unit { unit, children } => WidgetNode::Unit {
                unit: unit.clone(),
                children: Rc::clone(children),
            },
            WidgetNode::Group(g) => WidgetNode::Group(g.clone()),
        }
    }
}

pub struct WidgetNodeGroup<B: UiBackend> {
    render_order: Vec<IntOrString>,
    ordered: Vec<WidgetNode<B>>,
    named: HashMap<String, WidgetNode<B>>,
}

impl<B: UiBackend> Clone for WidgetNodeGroup<B> {
    fn clone(&self) -> Self {
        Self {
            render_order: self.render_order.clone(),
            ordered: self.ordered.clone(),
            named: self.named.clone(),
        }
    }
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
    fn unmount(self, ctx: &mut InternalContext<B>) {
        self.named
            .into_iter()
            .map(|(_, v)| v)
            .chain(self.ordered)
            .for_each(|node| node.unmount(ctx));
    }

    fn diff(&mut self, new: &WidgetNodeGroup<B>, ctx: &mut InternalContext<B>) {
        let WidgetNodeGroup {
            named: mut new_named,
            ordered: new_ordered,
            render_order: new_render_order,
        } = new.clone();
        self.render_order = new_render_order;
        if new_ordered.len() < self.ordered.len() {
            self.ordered
                .drain(new_ordered.len()..)
                .for_each(|w| w.unmount(ctx));
        }

        self.ordered
            .iter_mut()
            .zip(new_ordered)
            .for_each(|(old, new)| old.diff(&new, ctx));

        let ctx = RefCell::new(ctx);
        self.named
            .drain_filter(|name, old| {
                new_named
                    .remove(name)
                    .map(|new| old.diff(&new, *ctx.borrow_mut()))
                    .is_none()
            })
            .for_each(|(_, w)| w.unmount(*ctx.borrow_mut()));
    }

    fn process(&mut self, ctx: &mut InternalContext<B>) {
        self.named
            .iter_mut()
            .map(|(_, v)| v)
            .chain(&mut self.ordered)
            .for_each(|node| node.process(ctx));
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

    fn mount(&self, ctx: &mut InternalContext<B>) -> MountedWidgetNodeGroup<B> {
        let Self {
            named,
            ordered,
            render_order,
        } = self;
        MountedWidgetNodeGroup {
            named: named
                .into_iter()
                .map(|(n, w)| (n.clone(), w.mount(ctx)))
                .collect(),
            ordered: ordered.into_iter().map(|w| w.mount(ctx)).collect(),
            render_order: render_order.clone(),
        }
    }
}

enum MountedWidgetNode<B: UiBackend> {
    None,
    Component(MountedWidgetComponent<B>),
    Unit {
        unit: B::Unit,
        children: Box<MountedWidgetNode<B>>,
    },
    Group(MountedWidgetNodeGroup<B>),
}

impl<B: UiBackend> MountedWidgetNode<B> {
    fn diff(&mut self, new: &WidgetNode<B>, ctx: &mut InternalContext<B>) {
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
                let mut ectx = Context::<B> {
                    backend_data: &mut ctx.backend_data,
                };
                let effects = ctx.effects;
                let prop_recalc_effects = ctx.prop_recalc_effects;
                B::diff_units(unit, new_unit, &mut ectx, |ectx| {
                    let mut ctx = InternalContext {
                        backend_data: ectx.backend_data,
                        effects,
                        prop_recalc_effects,
                    };

                    children.diff(&new_children, &mut ctx);
                });
            }
            (MountedWidgetNode::Group(old), WidgetNode::Group(new)) => old.diff(new, ctx),
            (this, new) => std::mem::replace(this, new.mount(ctx)).unmount(ctx),
        }
    }

    fn unmount(self, ctx: &mut InternalContext<B>) {
        match self {
            MountedWidgetNode::None => {}
            MountedWidgetNode::Component(c) => c.unmount(ctx),
            MountedWidgetNode::Unit { children, .. } => children.unmount(ctx),
            MountedWidgetNode::Group(group) => group.unmount(ctx),
        }
    }

    fn process(&mut self, ctx: &mut InternalContext<B>) {
        match self {
            MountedWidgetNode::None => {}
            MountedWidgetNode::Component(c) => c.process(ctx, false),
            MountedWidgetNode::Unit { children, .. } => children.process(ctx),
            MountedWidgetNode::Group(group) => group.process(ctx),
        }
    }
}

struct MountedWidgetComponent<B: UiBackend> {
    template: WidgetComponent<B>,
    result: Box<MountedWidgetNode<B>>,
    data: MountedWidgetData<B>,
}

pub struct MountedWidgetData<B: UiBackend> {
    init_data: Box<dyn Any>,
    deplist: DynDepList<B>,
    all_effect_ids: Vec<u32>,
    propped_effect_ids: Vec<u32>,
}

impl<B: UiBackend> MountedWidgetComponent<B> {
    fn process(&mut self, ctx: &mut InternalContext<B>, force_recalc: bool) {
        if !force_recalc && !self.needs_recalc(ctx) {
            self.result.process(ctx);
            return;
        }

        self.data.deplist.clear();
        let new_result = self
            .template
            .func
            .call(ctx, &*self.template.props, &mut self.data);

        self.result.diff(&new_result, ctx);
    }

    fn needs_recalc(&mut self, ctx: &mut InternalContext<B>) -> bool {
        self.template.func.needs_recalc(ctx, &*self.data.init_data) || {
            let sctx = Context::<B> {
                backend_data: ctx.backend_data,
            };
            self.data.deplist.iter().any(|f| f(&sctx))
        }
    }

    fn diff(&mut self, new: &WidgetComponent<B>, ctx: &mut InternalContext<B>) {
        if self.template.func.fn_type_id() == new.func.fn_type_id() {
            self.template.props = Rc::clone(&new.props);
            self.process(ctx, true);
        } else {
            std::mem::replace(self, new.mount(ctx)).unmount(ctx)
        }
    }

    fn unmount(self, ctx: &mut InternalContext<B>) {
        self.result.unmount(ctx);
        self.template.func.deinit(ctx, self.data.init_data);
        for effect_id in self.data.all_effect_ids {
            let effect = ctx.effects.effects.remove(&effect_id).unwrap();
            let mut ctx = Context::<B> {
                backend_data: &mut ctx.backend_data,
            };
            (effect.cleanup.unwrap())(&mut ctx);
        }
    }
}

pub type DynDepList<B> = Vec<Box<dyn Fn(&Context<B>) -> bool>>;

pub trait UiBackend: Sized + 'static {
    type Unit: Clone;
    type RunCtx<'a>;
    type WidgetCtx<'a>;

    fn make_wctx<'a>(ctx: &'a Context<'_, '_, Self>) -> Self::WidgetCtx<'a>;

    /// You *must* call diff_children!
    fn diff_units(
        old: &mut Self::Unit,
        new: &Self::Unit,
        ctx: &mut Context<Self>,
        diff_children: impl FnOnce(&mut Context<Self>),
    );

    fn mark_update(ctx: &mut Self::RunCtx<'_>) {}
}

pub trait WidgetParam<B: UiBackend>: 'static {
    type InitData: 'static;
    type Item<'a>;

    fn init(ctx: &mut Context<B>) -> Self::InitData;
    fn deinit(ctx: &mut Context<B>, init_data: Self::InitData);
    fn get_item<'a>(ctx: &'a Context<B>, init_data: &mut Self::InitData) -> Self::Item<'a>;
    fn needs_recalc(ctx: &B::RunCtx<'_>, init_data: &Self::InitData) -> bool;
}

macro_rules! impl_functions {
    () => {
        impl_functions!(@single_row);
    };
    ($transfer: ident $(, $tail: ident)*) => {
        impl_functions!(@single_row $transfer $(, $tail)*);
        impl_functions!($($tail),*);
    };
    (@single_row $($idents: ident),*) => {
        impl_functions!(@private [], [$($idents),*]);
    };
    (@private [$($head: ident),*], []) => {
        impl_functions!(@finalize [$($head),*], []);
        impl_functions!(@finalize [$($head),*], [], WidgetContext);
    };
    (@private [$($head: ident),*], [$last: ident]) => {
        impl_functions!(@private [$($head,)* $last], []);
        impl_functions!(@finalize [$($head),*], [$last]);
        impl_functions!(@finalize [$($head),*], [$last], WidgetContext);
    };
    (@private [$($head: ident),*], [$transfer: ident, $($tail: ident),*]) => {
        impl_functions!(@private [$($head,)* $transfer], [$($tail),*]);
        impl_functions!(@finalize [$($head),*], [$($tail,)* $transfer]);
        impl_functions!(@finalize [$($head),*], [$($tail,)* $transfer], WidgetContext);
    };
    (@if exists!() then ($($t:tt)*) else ($($e:tt)*)) => { $($e)* };
    (@if exists!($($c:tt)*) then ($($t:tt)*) else ($($e:tt)*)) => { $($t)* };
    (@exists $($x:tt)*) => { Yes };
    (@finalize [$($props: ident),*], [$($params: ident),*] $(, $context: ident)?) => {
        #[allow(unused)]
        #[allow(non_snake_case)]
        impl<Backend: UiBackend, Func, $($props,)* $($params,)*>
            WidgetFunc<($($props,)*), Backend, (impl_functions!(@if exists!($($context)?) then (Yes) else (No)), $($params,)*)>
            for Func
        where
            Func: Fn($($context<Backend>,)? $(&$props,)* $($params,)*) -> WidgetNode<Backend> + Copy + 'static,
            Func: for<'a> Fn($($context<Backend>,)? $(&$props,)* $(<$params as WidgetParam<Backend>>::Item<'a>,)*) -> WidgetNode<Backend> + Copy + 'static,
            $($props: 'static,)*
            $($params: WidgetParam<Backend>,)*
        {
            fn call(&self, ctx: &mut InternalContext<Backend>, ($($props,)*): &($($props,)*), data: &mut MountedWidgetData<Backend>) -> WidgetNode<Backend> {
                let mut init_data = &mut *data.init_data;
                let ($($params,)*): &mut ($(<$params as WidgetParam<Backend>>::InitData,)*) = init_data.downcast_mut().unwrap();
                let sctx = Context {
                    backend_data: ctx.backend_data,
                };
                let mut wctx = Backend::make_wctx(&sctx);
                let w_wctx = WidgetContext::<Backend> {
                    backend_data: &mut wctx,
                    effects: &mut ctx.effects,
                    deplist: &mut data.deplist,
                    all_effect_ids: &mut data.all_effect_ids,
                    propped_effect_ids: &mut data.propped_effect_ids,
                };
                let result = impl_functions!(@if exists!($($context)?) then (
                    (self)(
                        w_wctx,
                        $($props,)*
                        $($params::get_item(&sctx, $params),)*
                    )
                ) else (
                    (self)(
                        $($props,)*
                        $($params::get_item(&sctx, $params),)*
                    )
                ));

                result
            }

            fn needs_recalc(&self, ctx: &InternalContext<Backend>, init_data: &dyn Any) -> bool {
                let ($($params,)*): &($(<$params as WidgetParam<Backend>>::InitData,)*) = init_data.downcast_ref().unwrap();
                false $(|| $params::needs_recalc(&ctx.backend_data, $params))*
            }

            fn init(&self, ctx: &mut InternalContext<Backend>) -> Box<dyn Any> {
                let mut sctx = Context::<Backend> {
                    backend_data: ctx.backend_data,
                };
                Box::new(($($params::init(&mut sctx),)*))
            }

            fn deinit(&self, ctx: &mut InternalContext<Backend>, init_data: Box<dyn Any>) {
                let mut sctx = Context::<Backend> {
                    backend_data: ctx.backend_data,
                };
                let ($($params,)*): ($(<$params as WidgetParam<Backend>>::InitData,)*) = *init_data.downcast().unwrap();
                $($params::deinit(&mut sctx, $params);)*
            }

            fn as_dynamic(&self) -> Rc<dyn DynWidgetFunc<Backend>> {
                Rc::new(
                    Box::new(*self) as Box<dyn
                        WidgetFunc<($($props,)*), Backend, (impl_functions!(@if exists!($($context)?) then (Yes) else (No)), $($params,)*)>
                    >
                )
            }

            fn fn_type_id(&self) -> TypeId {
                TypeId::of::<Func>()
            }
        }
    };
}

impl_functions!(
    _0, _1,
    _2 //, _3, _4, _5, _6, _7, _8, _9, _10, _11, _12, _13, _14, _15, _16, _17, _18, _19, _20
);

type Yes = Checker<true>;
type No = Checker<false>;

pub struct Checker<const EXISTS: bool>;
