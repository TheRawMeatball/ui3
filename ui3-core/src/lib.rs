#![feature(hash_drain_filter)]
#![feature(generic_associated_types)]

use std::{
    any::{Any, TypeId},
    cell::{Cell, RefCell},
    collections::HashMap,
    marker::PhantomData,
    ops::DerefMut,
    rc::Rc,
};

pub struct Application<B: UiBackend> {
    root: MountedWidgetNode<B>,
}

impl<B: UiBackend> Application<B> {
    pub fn update(&mut self, ctx: &mut B::RunCtx<'_>) {
        let mut execute_at_end = vec![];
        loop {
            let mut ctx = InternalContext {
                backend_data: &mut *ctx,
                execute_at_end: &mut execute_at_end,
            };
            self.root.process(&mut ctx);
            let execute_at_end = ctx.execute_at_end;
            let ctx = ctx.backend_data;
            B::mark_update(ctx);
            if execute_at_end.is_empty() {
                break;
            }
            execute_at_end.drain(..).for_each(|f| f(ctx));
        }
    }

    pub fn new(root: WidgetNode<B>, ctx: &mut B::RunCtx<'_>) -> Self {
        let mut execute_at_end = vec![];

        let backend_data = ctx;
        let mut ctx = InternalContext {
            backend_data,
            execute_at_end: &mut execute_at_end,
        };
        let mut this = Self {
            root: root.mount(&mut ctx),
        };
        let execute_at_end = ctx.execute_at_end;
        let ctx = ctx.backend_data;
        execute_at_end.drain(..).for_each(|f| f(ctx));
        B::mark_update(backend_data);
        this.update(backend_data);
        this
    }

    pub fn render(&self) -> Vec<RenderNode<B>> {
        self.root.render()
    }
}

pub struct RenderNode<'a, B: UiBackend> {
    pub unit: &'a B::Unit,
    pub children: Vec<RenderNode<'a, B>>,
}

#[doc(hidden)]
pub struct InternalContext<'b, 'ctx, B: UiBackend> {
    backend_data: &'b mut B::RunCtx<'ctx>,
    execute_at_end: &'b mut Vec<Box<dyn FnOnce(&mut B::RunCtx<'_>)>>,
}

pub trait WidgetFunc<P: 'static, B: UiBackend, Marker>: 'static {
    fn call(
        &self,
        ctx: &mut InternalContext<B>,
        props: &P,
        init_data: &mut dyn Any,
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

pub trait EffectFunc<P: 'static, B: UiBackend, Marker>: 'static {
    fn call(
        &self,
        ctx: &mut B::RunCtx<'_>,
        props: &P,
        init_data: &mut dyn Any,
    ) -> Box<dyn FnOnce(&mut B::RunCtx<'_>)>;
    fn init(&self, ctx: &mut B::RunCtx<'_>) -> Rc<RefCell<dyn Any>>;
    fn needs_recalc(&self, ctx: &B::RunCtx<'_>, init_data: &dyn Any) -> bool;
    fn as_dynamic(&self) -> Rc<dyn DynEffectFunc<B>>;
    fn fn_type_id(&self) -> TypeId;
    fn e(self, props: P) -> WidgetNode<B>
    where
        Self: Sized,
    {
        WidgetNode::Effect(WidgetEffectComponent {
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
        init_data: &mut dyn Any,
    ) -> WidgetNode<B>;
    fn needs_recalc(&self, ctx: &InternalContext<B>, init_data: &dyn Any) -> bool;
    fn init(&self, ctx: &mut InternalContext<B>) -> Box<dyn Any>;
    fn deinit(&self, ctx: &mut InternalContext<B>, init_data: Box<dyn Any>);
    fn fn_type_id(&self) -> TypeId;
}
pub trait DynEffectFunc<B: UiBackend>: 'static {
    fn call(
        &self,
        ctx: &mut B::RunCtx<'_>,
        props: &dyn Any,
        init_data: &mut dyn Any,
    ) -> Box<dyn FnOnce(&mut B::RunCtx<'_>)>;
    fn needs_recalc(&self, ctx: &B::RunCtx<'_>, init_data: &dyn Any) -> bool;
    fn init(&self, ctx: &mut B::RunCtx<'_>) -> Rc<RefCell<dyn Any>>;
    fn fn_type_id(&self) -> TypeId;
}

impl<P: 'static, B: UiBackend, Params: 'static> DynWidgetFunc<B>
    for Box<dyn WidgetFunc<P, B, Params>>
{
    fn call(
        &self,
        ctx: &mut InternalContext<B>,
        props: &dyn Any,
        data: &mut dyn Any,
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

impl<P: 'static, B: UiBackend, Params: 'static> DynEffectFunc<B>
    for Box<dyn EffectFunc<P, B, Params>>
{
    fn call(
        &self,
        ctx: &mut B::RunCtx<'_>,
        props: &dyn Any,
        data: &mut dyn Any,
    ) -> Box<dyn FnOnce(&mut B::RunCtx<'_>)> {
        (**self).call(ctx, props.downcast_ref().unwrap(), data)
    }

    fn needs_recalc(&self, ctx: &B::RunCtx<'_>, init_data: &dyn Any) -> bool {
        (**self).needs_recalc(ctx, init_data)
    }

    fn fn_type_id(&self) -> TypeId {
        (**self).fn_type_id()
    }

    fn init(&self, stores: &mut B::RunCtx<'_>) -> Rc<RefCell<dyn Any>> {
        (**self).init(stores)
    }
}

pub struct WidgetComponent<B: UiBackend> {
    func: Rc<dyn DynWidgetFunc<B>>,
    props: Rc<dyn Any>,
}

pub struct WidgetEffectComponent<B: UiBackend> {
    func: Rc<dyn DynEffectFunc<B>>,
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

impl<B: UiBackend> Clone for WidgetEffectComponent<B> {
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
            WidgetNode::Component(c) => MountedWidgetNode::Component(c.mount(ctx)),
            WidgetNode::Effect(c) => MountedWidgetNode::Effect(c.mount(ctx)),
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
        let mut init_data = self.func.init(ctx);
        let result = self.func.call(ctx, &*self.props, &mut *init_data);
        let result = result.mount(ctx);

        MountedWidgetComponent {
            template: self.clone(),
            result: Box::new(result),
            init_data,
        }
    }
}

impl<B: UiBackend> WidgetEffectComponent<B> {
    fn mount(&self, ctx: &mut InternalContext<B>) -> MountedWidgetEffectComponent<B> {
        let init_data = self.func.init(ctx.backend_data);
        // self.func.call(ctx, &*self.props, &mut *init_data);

        MountedWidgetEffectComponent {
            template: self.clone(),
            init_data,
            cleanup_fn: Rc::new(Cell::new(Box::new(|_| {}))),
        }
    }
}

pub enum WidgetNode<B: UiBackend> {
    None,
    Component(WidgetComponent<B>),
    Effect(WidgetEffectComponent<B>),
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
            WidgetNode::Effect(c) => WidgetNode::Effect(c.clone()),
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
    Effect(MountedWidgetEffectComponent<B>),
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
            (MountedWidgetNode::Effect(c), WidgetNode::Effect(new)) => c.diff(new, ctx),
            (
                MountedWidgetNode::Unit { unit, children },
                WidgetNode::Unit {
                    unit: new_unit,
                    children: new_children,
                },
            ) => {
                *unit = new_unit.clone();
                children.diff(&new_children, ctx);
            }
            (MountedWidgetNode::Group(old), WidgetNode::Group(new)) => old.diff(new, ctx),
            (this, new) => std::mem::replace(this, new.mount(ctx)).unmount(ctx),
        }
    }

    fn unmount(self, ctx: &mut InternalContext<B>) {
        match self {
            MountedWidgetNode::None => {}
            MountedWidgetNode::Component(c) => c.unmount(ctx),
            MountedWidgetNode::Effect(c) => c.unmount(ctx),
            MountedWidgetNode::Unit { children, .. } => children.unmount(ctx),
            MountedWidgetNode::Group(group) => group.unmount(ctx),
        }
    }

    fn process(&mut self, ctx: &mut InternalContext<B>) {
        match self {
            MountedWidgetNode::None => {}
            MountedWidgetNode::Component(c) => c.process(ctx, false),
            MountedWidgetNode::Effect(c) => c.process(ctx, false),
            MountedWidgetNode::Unit { children, .. } => children.process(ctx),
            MountedWidgetNode::Group(group) => group.process(ctx),
        }
    }

    fn render(&self) -> Vec<RenderNode<B>> {
        match self {
            MountedWidgetNode::None | MountedWidgetNode::Effect(_) => vec![],
            MountedWidgetNode::Component(c) => c.result.render(),
            MountedWidgetNode::Unit { unit, children } => vec![RenderNode {
                unit,
                children: children.render(),
            }],
            MountedWidgetNode::Group(g) => g
                .render_order
                .iter()
                .flat_map(|ios| match ios {
                    IntOrString::Int(i) => g.ordered[*i].render(),
                    IntOrString::String(s) => g.named[s].render(),
                })
                .collect(),
        }
    }
}

struct MountedWidgetComponent<B: UiBackend> {
    template: WidgetComponent<B>,
    result: Box<MountedWidgetNode<B>>,
    init_data: Box<dyn Any>,
}

struct MountedWidgetEffectComponent<B: UiBackend> {
    template: WidgetEffectComponent<B>,
    init_data: Rc<RefCell<dyn Any>>,
    cleanup_fn: Rc<Cell<Box<dyn FnOnce(&mut B::RunCtx<'_>)>>>,
}

impl<B: UiBackend> MountedWidgetComponent<B> {
    fn process(&mut self, ctx: &mut InternalContext<B>, force_recalc: bool) {
        if !force_recalc && !self.needs_recalc(ctx) {
            self.result.process(ctx);
            return;
        }

        let new_result = self
            .template
            .func
            .call(ctx, &*self.template.props, &mut *self.init_data);

        self.result.diff(&new_result, ctx);
    }

    fn needs_recalc(&mut self, ctx: &mut InternalContext<B>) -> bool {
        self.template.func.needs_recalc(ctx, &*self.init_data)
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
        self.template.func.deinit(ctx, self.init_data);
    }
}

impl<B: UiBackend> MountedWidgetEffectComponent<B> {
    fn process(&mut self, ctx: &mut InternalContext<B>, force_recalc: bool) {
        if force_recalc || self.needs_recalc(ctx) {
            let cleanup_fn = self.cleanup_fn.clone();
            let template = self.template.clone();
            let init_data = self.init_data.clone();
            ctx.execute_at_end.push(Box::new(move |ctx| {
                (cleanup_fn.replace(Box::new(|_| {})))(ctx);
                let new_cleanup_fn =
                    template
                        .func
                        .call(ctx, &*template.props, &mut *init_data.borrow_mut());
                cleanup_fn.set(new_cleanup_fn);
            }))
        }
    }

    fn needs_recalc(&mut self, ctx: &mut InternalContext<B>) -> bool {
        self.template
            .func
            .needs_recalc(ctx.backend_data, &*self.init_data.borrow())
    }

    fn diff(&mut self, new: &WidgetEffectComponent<B>, ctx: &mut InternalContext<B>) {
        if self.template.func.fn_type_id() == new.func.fn_type_id() {
            self.template.props = Rc::clone(&new.props);
            self.process(ctx, true);
        }
    }

    fn unmount(self, ctx: &mut InternalContext<B>) {
        ctx.execute_at_end
            .push(self.cleanup_fn.replace(Box::new(|_| {})))
    }
}

pub type DynDepList<B> = Vec<Box<dyn Fn(&<B as UiBackend>::RunCtx<'_>) -> bool>>;

pub trait UiBackend: Sized + 'static {
    type Unit: Clone;
    type RunCtx<'a>;

    fn mark_update(ctx: &mut Self::RunCtx<'_>);

    // Store support
    type StoreId: Copy + 'static;
    type TrackingPtr<'a, T: Send + Sync + 'static>: DerefMut<Target = T>;
    type StoreInitData;
    fn access_store_mut<'a, T: Send + Sync + 'static>(
        id: Self::StoreId,
        ctx: &'a mut Self::RunCtx<'_>,
    ) -> Self::TrackingPtr<'a, T>;
    fn access_store<'a, T: Send + Sync + 'static>(
        id: Self::StoreId,
        ctx: &'a Self::RunCtx<'_>,
    ) -> &'a T;
    fn read_store_marked<'a, T: Send + Sync + 'static>(
        id: Self::StoreId,
        ctx: &'a Self::RunCtx<'_>,
        init_data: &mut Self::StoreInitData,
    ) -> &'a T;
    fn init_store<T: Send + Sync + 'static>(
        ctx: &mut Self::RunCtx<'_>,
        val: T,
    ) -> Self::StoreInitData;
    fn deinit_store(data: Self::StoreInitData, ctx: &mut Self::RunCtx<'_>);
    fn id_from_store_init_data(data: &Self::StoreInitData) -> Self::StoreId;
    fn check_store_needs_recalc<T: Send + Sync + 'static>(
        ctx: &Self::RunCtx<'_>,
        init_data: &Self::StoreInitData,
    ) -> bool;
}

pub struct Store<'a, T, B: UiBackend> {
    val: &'a T,
    id: B::StoreId,
}

impl<'a, T, B: UiBackend> Store<'a, T, B> {
    pub fn id(&self) -> StoreId<T, B> {
        StoreId {
            id: self.id,
            _m: PhantomData,
        }
    }
}

impl<'a, T, B: UiBackend> std::ops::Deref for Store<'a, T, B> {
    type Target = &'a T;

    fn deref(&self) -> &Self::Target {
        &self.val
    }
}

pub struct StoreId<T, B: UiBackend> {
    pub id: B::StoreId,
    _m: PhantomData<T>,
}

impl<T: Send + Sync + 'static, B: UiBackend> StoreId<T, B> {
    pub fn access_mut<'a>(self, ctx: &'a mut B::RunCtx<'_>) -> B::TrackingPtr<'a, T> {
        B::access_store_mut(self.id, ctx)
    }
}

impl<T, B: UiBackend> Clone for StoreId<T, B> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            _m: PhantomData,
        }
    }
}

impl<T, B: UiBackend> Copy for StoreId<T, B> {}

impl<B: UiBackend, T: Default + Send + Sync + 'static> WidgetParam<B> for Store<'static, T, B> {
    type InitData = B::StoreInitData;

    type Item<'ctx, 's> = Store<'ctx, T, B>;

    fn init(ctx: &mut B::RunCtx<'_>) -> B::StoreInitData {
        B::init_store(ctx, T::default())
    }

    fn deinit(ctx: &mut B::RunCtx<'_>, init_data: Self::InitData) {
        B::deinit_store(init_data, ctx)
    }

    fn get_item<'ctx, 's>(
        ctx: &'ctx B::RunCtx<'_>,
        init_data: &'s mut Self::InitData,
    ) -> Self::Item<'ctx, 's> {
        let id = B::id_from_store_init_data(&init_data);
        Store {
            val: &B::read_store_marked(id, ctx, init_data),
            id,
        }
    }

    fn needs_recalc(ctx: &B::RunCtx<'_>, init_data: &Self::InitData) -> bool {
        B::check_store_needs_recalc::<T>(ctx, init_data)
    }
}

pub trait WidgetParam<B: UiBackend>: 'static {
    type InitData: 'static;
    type Item<'ctx, 's>;

    fn init(ctx: &mut B::RunCtx<'_>) -> Self::InitData;
    fn deinit(ctx: &mut B::RunCtx<'_>, init_data: Self::InitData);
    fn get_item<'ctx, 's>(
        ctx: &'ctx B::RunCtx<'_>,
        init_data: &'s mut Self::InitData,
    ) -> Self::Item<'ctx, 's>;
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
    };
    (@private [$($head: ident),*], [$last: ident]) => {
        impl_functions!(@private [$($head,)* $last], []);
        impl_functions!(@finalize [$($head),*], [$last]);
    };
    (@private [$($head: ident),*], [$transfer: ident, $($tail: ident),*]) => {
        impl_functions!(@private [$($head,)* $transfer], [$($tail),*]);
        impl_functions!(@finalize [$($head),*], [$($tail,)* $transfer]);
    };
    (@exists $($x:tt)*) => { Yes };
    (@finalize [$($props: ident),*], [$($params: ident),*]) => {
        #[allow(unused)]
        #[allow(non_snake_case)]
        impl<Backend: UiBackend, Func, $($props,)* $($params,)*>
            WidgetFunc<($($props,)*), Backend, ($($params,)*)>
            for Func
        where
            Func: Fn($(&$props,)* $($params,)*) -> WidgetNode<Backend> + Copy + 'static,
            Func: for<'ctx, 's> Fn($(&$props,)* $(<$params as WidgetParam<Backend>>::Item<'ctx, 's>,)*) -> WidgetNode<Backend> + Copy + 'static,
            $($props: 'static,)*
            $($params: WidgetParam<Backend>,)*
        {
            fn call(&self, ctx: &mut InternalContext<Backend>, ($($props,)*): &($($props,)*), init_data: &mut dyn Any) -> WidgetNode<Backend> {
                let ($($params,)*): &mut ($(<$params as WidgetParam<Backend>>::InitData,)*) = init_data.downcast_mut().unwrap();

                (self)(
                    $($props,)*
                    $($params::get_item(&ctx.backend_data, $params),)*
                )
            }

            fn needs_recalc(&self, ctx: &InternalContext<Backend>, init_data: &dyn Any) -> bool {
                let ($($params,)*): &($(<$params as WidgetParam<Backend>>::InitData,)*) = init_data.downcast_ref().unwrap();
                false $(|| $params::needs_recalc(&ctx.backend_data, $params))*
            }

            fn init(&self, ctx: &mut InternalContext<Backend>) -> Box<dyn Any> {
                Box::new(($($params::init(&mut ctx.backend_data),)*))
            }

            fn deinit(&self, ctx: &mut InternalContext<Backend>, init_data: Box<dyn Any>) {
                let ($($params,)*): ($(<$params as WidgetParam<Backend>>::InitData,)*) = *init_data.downcast().unwrap();
                $($params::deinit(&mut ctx.backend_data, $params);)*
            }

            fn as_dynamic(&self) -> Rc<dyn DynWidgetFunc<Backend>> {
                Rc::new(
                    Box::new(*self) as Box<dyn
                        WidgetFunc<($($props,)*), Backend, ($($params,)*)>
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
    _0, _1, _2, _3, _4, _5, _6, _7, _8, _9, _10, _11, _12, _13, _14, _15, _16, _17, _18, _19, _20
);
