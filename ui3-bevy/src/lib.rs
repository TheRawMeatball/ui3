#![feature(generic_associated_types)]

use std::rc::Rc;

use bevy::{
    ecs::prelude::*,
    prelude::{Color, Handle, Texture},
    text::Text,
    ui::Style,
};
use ui3_core::{UiBackend, WidgetParam};

pub struct BevyBackend;

#[repr(transparent)]
struct UiStoreWrapper<T>(T);

impl UiBackend for BevyBackend {
    type Unit = Unit;

    type RunCtx<'a> = World;

    fn mark_update(ctx: &mut Self::RunCtx<'_>) {
        ctx.increment_change_tick();
    }

    type StoreId = Entity;

    type TrackingPtr<'a, T: Send + Sync + 'static> = Mut<'a, T>;

    type StoreInitData = (Entity, u32);

    fn access_store_mut<'a, T: Send + Sync + 'static>(
        id: Self::StoreId,
        ctx: &'a mut Self::RunCtx<'_>,
    ) -> Self::TrackingPtr<'a, T> {
        let raw: Mut<UiStoreWrapper<T>> = ctx.get_mut(id).unwrap();
        // should be safe because of #[repr(transparent)]
        // this is why Mut should be mappable.
        unsafe { std::mem::transmute::<Mut<'a, UiStoreWrapper<T>>, Mut<'a, T>>(raw) }
    }

    fn access_store<'a, T: Send + Sync + 'static>(
        id: Self::StoreId,
        ctx: &'a Self::RunCtx<'_>,
    ) -> &'a T {
        &ctx.get::<UiStoreWrapper<T>>(id).unwrap().0
    }

    fn read_store_marked<'a, T: Send + Sync + 'static>(
        id: Self::StoreId,
        ctx: &'a Self::RunCtx<'_>,
        init_data: &mut Self::StoreInitData,
    ) -> &'a T {
        init_data.1 = ctx.read_change_tick();
        Self::access_store(id, ctx)
    }

    fn init_store<T: Send + Sync + 'static>(
        ctx: &mut Self::RunCtx<'_>,
        val: T,
    ) -> Self::StoreInitData {
        (ctx.spawn().insert(UiStoreWrapper(val)).id(), 0)
    }

    fn deinit_store(data: Self::StoreInitData, ctx: &mut Self::RunCtx<'_>) {
        ctx.despawn(data.0);
    }

    fn id_from_store_init_data(data: &Self::StoreInitData) -> Self::StoreId {
        data.0
    }

    fn check_store_needs_recalc<T: Send + Sync + 'static>(
        ctx: &Self::RunCtx<'_>,
        init_data: &Self::StoreInitData,
    ) -> bool {
        ctx.entity(init_data.0)
            .get_change_ticks::<UiStoreWrapper<T>>()
            .unwrap()
            .is_changed(init_data.1, ctx.read_change_tick())
    }
}

pub mod prelude {
    use super::BevyBackend;

    pub use ui3_core::WidgetFunc;

    pub type UiApp = ui3_core::Application<BevyBackend>;
    pub type WidgetNode = ui3_core::WidgetNode<BevyBackend>;

    pub use crate::UiRes;
}

#[derive(Clone)]
pub enum Unit {
    Node {
        style: Style,
        color: Color,
        image: Option<Handle<Texture>>,
    },
    Button {
        func: Rc<dyn Fn(&mut World)>,
        style: Style,
        color: Color,
        image: Option<Handle<Texture>>,
    },
    Text {
        style: Style,
        text: Text,
    },
}

pub struct UiRes<'a, T> {
    v: &'a T,
}

impl<'a, T> std::ops::Deref for UiRes<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.v
    }
}

impl<'a, T: Send + Sync + 'static> WidgetParam<BevyBackend> for UiRes<'static, T> {
    type InitData = u32;

    type Item<'ctx, 's> = UiRes<'ctx, T>;

    fn init(ctx: &mut World) -> Self::InitData {
        ctx.read_change_tick()
    }

    fn deinit(_: &mut World, _: Self::InitData) {}

    fn get_item<'ctx, 's>(
        ctx: &'ctx World,
        init_data: &'s mut Self::InitData,
    ) -> Self::Item<'ctx, 's> {
        *init_data = ctx.read_change_tick();
        UiRes {
            v: ctx.get_resource().unwrap(),
        }
    }

    fn needs_recalc(ctx: &World, init_data: &Self::InitData) -> bool {
        ctx.get_resource_change_ticks::<T>()
            .is_changed(*init_data, ctx.read_change_tick())
    }
}
