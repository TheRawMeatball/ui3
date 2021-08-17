use std::rc::Rc;

use bevy::prelude::*;
use send_wrapper::SendWrapper;
use ui3_bevy::{prelude::*, BevyBackend, Unit};
use ui3_core::RenderNode;

fn main() {
    println!("hi");
    let mut app = App::build();
    app.add_plugins(DefaultPlugins);
    let ui_font = UiFont(
        app.world_mut()
            .get_resource::<AssetServer>()
            .unwrap()
            .load("FiraMono-Medium.ttf"),
    );
    app.insert_resource(ui_font);
    app.add_system(ui_system.exclusive_system());

    app.world_mut()
        .spawn()
        .insert_bundle(UiCameraBundle::default());
    let ui_app = UiApp::new(rootw.w(()), app.world_mut());
    app.insert_non_send_resource(ui_app);

    app.run();
}

struct UiFont(Handle<Font>);

struct UiElement;

struct ButtonFunc(SendWrapper<Rc<dyn Fn(&mut World)>>);

fn ui_system(world: &mut World) {
    let mut app = world.remove_non_send::<UiApp>().unwrap();
    app.update(world);
    let list = world
        .query_filtered::<Entity, With<UiElement>>()
        .iter(world)
        .collect::<Vec<_>>();
    for entity in list {
        world.despawn(entity);
    }

    let root_id = world
        .spawn()
        .insert_bundle(NodeBundle {
            ..Default::default()
        })
        .insert(UiElement)
        .id();

    fn process(node: RenderNode<BevyBackend>, world: &mut World, parent: Entity) {
        let id = match node.unit {
            ui3_bevy::Unit::Node {
                style,
                color,
                image,
            } => {
                let bundle = NodeBundle {
                    style: style.clone(),
                    material: world
                        .get_resource_mut::<Assets<ColorMaterial>>()
                        .unwrap()
                        .add(if let Some(image) = image {
                            ColorMaterial::modulated_texture(image.clone(), color.clone())
                        } else {
                            color.clone().into()
                        }),
                    ..Default::default()
                };
                world.spawn().insert_bundle(bundle).id()
            }
            ui3_bevy::Unit::Text { style, text } => world
                .spawn()
                .insert_bundle(TextBundle {
                    style: style.clone(),
                    text: text.clone(),
                    ..Default::default()
                })
                .id(),
            Unit::Button {
                func,
                style,
                color,
                image,
            } => {
                let bundle = ButtonBundle {
                    style: style.clone(),
                    material: world
                        .get_resource_mut::<Assets<ColorMaterial>>()
                        .unwrap()
                        .add(if let Some(image) = image {
                            ColorMaterial::modulated_texture(image.clone(), color.clone())
                        } else {
                            color.clone().into()
                        }),
                    ..Default::default()
                };
                world
                    .spawn()
                    .insert(ButtonFunc(SendWrapper::new(func.clone())))
                    .insert_bundle(bundle)
                    .id()
            }
        };
        let id = world.entity_mut(parent).push_children(&[id]).id();
        for node in node.children {
            process(node, world, id);
        }
    }

    app.render()
        .into_iter()
        .for_each(|n| process(n, world, root_id));
    world.insert_non_send(app);
}

fn rootw() -> WidgetNode {
    textw.w(("hi!".into(),))
}

fn textw(text: &String, font: UiRes<UiFont>) -> WidgetNode {
    WidgetNode::Unit {
        unit: Unit::Text {
            style: Default::default(),
            text: Text::with_section(
                text.clone(),
                TextStyle {
                    font: font.0.clone(),
                    font_size: 50.0,
                    color: Color::BLACK,
                },
                Default::default(),
            ),
        },
        children: Rc::new(WidgetNode::None),
    }
}
