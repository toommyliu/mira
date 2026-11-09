use ruffle_core::{
    ContextMenuItem, Player, PlayerEvent,
    events::{KeyDescriptor, MouseButton},
};

use super::view::Point;
use crate::bridge::Bridge;

pub(crate) struct ContextMenu {
    pub(crate) items: Vec<ContextMenuItem>,
    pub(crate) at: Point,
    pub(crate) smooth_motion: bool,
    pub(crate) stats: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum MenuSelection {
    Ruffle(usize),
    SmoothMotion,
    Stats,
}

pub(super) struct OpenMenu {
    pub(super) items: Vec<ContextMenuItem>,
    x: f64,
    y: f64,
}

pub(super) struct PlayerInput {
    bridge: Bridge,
    keys: Vec<KeyDescriptor>,
}

impl PlayerInput {
    pub(super) fn new(bridge: Bridge) -> Self {
        Self {
            bridge,
            keys: Vec::new(),
        }
    }

    pub(super) fn handle_event(
        &mut self,
        player: &mut Player,
        event: PlayerEvent,
    ) -> Option<OpenMenu> {
        if !self.bridge.loaded()
            && matches!(
                event,
                PlayerEvent::MouseDown {
                    button: MouseButton::Right,
                    ..
                } | PlayerEvent::MouseUp {
                    button: MouseButton::Right,
                    ..
                }
            )
        {
            return None;
        }
        match &event {
            PlayerEvent::KeyDown { key } => {
                if !self.keys.iter().any(|held| {
                    held.physical_key == key.physical_key && held.key_location == key.key_location
                }) {
                    self.keys.push(*key);
                }
            }
            PlayerEvent::KeyUp { key } => self.keys.retain(|held| {
                held.physical_key != key.physical_key || held.key_location != key.key_location
            }),
            _ => {}
        }
        let position = match event {
            PlayerEvent::MouseDown {
                x,
                y,
                button: MouseButton::Right,
                ..
            } => Some((x, y)),
            _ => None,
        };
        let handled = player.handle_event(event);
        let (x, y) = position.filter(|_| !handled)?;
        self.release_keys(player);
        Some(OpenMenu {
            items: player.prepare_context_menu(),
            x,
            y,
        })
    }
}

impl PlayerInput {
    /// AppKit may consume key releases while it tracks a native menu, so a menu opening over
    /// the player releases every held key first.
    pub(super) fn release_keys(&mut self, player: &mut Player) {
        for key in self.keys.drain(..) {
            player.handle_event(PlayerEvent::KeyUp { key });
        }
    }
}

impl OpenMenu {
    pub(super) fn position(&self, scale: f64) -> Point {
        Point {
            x: (self.x / scale) as f32,
            y: (self.y / scale) as f32,
        }
    }

    pub(super) fn close(self, player: &mut Player, selection: Option<MenuSelection>) {
        match selection {
            Some(MenuSelection::Ruffle(index))
                if self.items.get(index).is_some_and(|item| item.enabled) =>
            {
                player.run_context_menu_callback(index);
            }
            Some(MenuSelection::SmoothMotion) => {
                player.set_frame_interpolation(!player.frame_interpolation());
            }
            _ => {}
        }
        player.clear_custom_menu_items();
        player.handle_event(PlayerEvent::MouseUp {
            x: self.x,
            y: self.y,
            button: MouseButton::Right,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruffle_core::{
        PlayerBuilder,
        events::{KeyCode, KeyDescriptor, KeyLocation, LogicalKey, NamedKey, PhysicalKey},
        external::ExternalInterfaceProvider,
        tag_utils::SwfMovie,
    };
    use ruffle_render::quality::StageQuality;

    fn right_click(input: &mut PlayerInput, player: &mut Player) -> OpenMenu {
        input
            .handle_event(
                player,
                PlayerEvent::MouseDown {
                    x: 40.0,
                    y: 60.0,
                    button: MouseButton::Right,
                    index: Some(0),
                },
            )
            .expect("right click opens the menu")
    }

    #[test]
    fn right_click_menu_runs_ruffle_callbacks_and_toggles_smooth_motion() {
        let movie = SwfMovie::from_data(
            include_bytes!("../../assets/loader.swf"),
            "file:///mira/loader.swf".into(),
            None,
            None,
        )
        .unwrap();
        let bridge = Bridge::default();
        let player = PlayerBuilder::new()
            .with_movie(movie)
            .with_external_interface(Box::new(bridge.clone()))
            .build();
        let mut player = player.lock().unwrap();
        let mut input = PlayerInput::new(bridge.clone());
        player.mutate_with_update_context(|context| bridge.call_method(context, "onProgress", &[]));
        assert!(
            input
                .handle_event(
                    &mut player,
                    PlayerEvent::MouseDown {
                        x: 40.0,
                        y: 60.0,
                        button: MouseButton::Right,
                        index: Some(0),
                    }
                )
                .is_none(),
            "the loading movie must not open a context menu"
        );
        assert!(
            !player.mutate_with_update_context(|context| context
                .input
                .is_mouse_down(MouseButton::Right))
        );
        player.mutate_with_update_context(|context| bridge.call_method(context, "onLoaded", &[]));
        player.set_quality(StageQuality::High);
        let menu = right_click(&mut input, &mut player);
        assert_eq!(menu.position(2.0), Point { x: 20.0, y: 30.0 });
        let low = menu
            .items
            .iter()
            .position(|item| item.caption == "Quality: Low")
            .unwrap();
        menu.close(&mut player, Some(MenuSelection::Ruffle(low)));
        assert_eq!(player.quality(), StageQuality::Low);

        right_click(&mut input, &mut player).close(&mut player, Some(MenuSelection::SmoothMotion));
        assert!(player.frame_interpolation());
        right_click(&mut input, &mut player).close(&mut player, None);
        assert!(player.frame_interpolation());
        right_click(&mut input, &mut player).close(&mut player, Some(MenuSelection::SmoothMotion));
        assert!(!player.frame_interpolation());
        for key in [
            KeyDescriptor {
                physical_key: PhysicalKey::KeyW,
                logical_key: LogicalKey::Character('w'),
                key_location: KeyLocation::Standard,
            },
            KeyDescriptor {
                physical_key: PhysicalKey::ShiftLeft,
                logical_key: LogicalKey::Named(NamedKey::Shift),
                key_location: KeyLocation::Standard,
            },
        ] {
            input.handle_event(&mut player, PlayerEvent::KeyDown { key });
        }
        assert!(player.mutate_with_update_context(|context| {
            context.input.is_key_down(KeyCode::W) && context.input.is_key_down(KeyCode::SHIFT)
        }));
        let menu = right_click(&mut input, &mut player);
        // AppKit can consume the key releases while tracking the menu.
        menu.close(&mut player, None);
        assert!(player.mutate_with_update_context(|context| {
            !context.input.is_key_down(KeyCode::W) && !context.input.is_key_down(KeyCode::SHIFT)
        }));
        let w = KeyDescriptor {
            physical_key: PhysicalKey::KeyW,
            logical_key: LogicalKey::Character('w'),
            key_location: KeyLocation::Standard,
        };
        input.handle_event(&mut player, PlayerEvent::KeyDown { key: w });
        input.release_keys(&mut player);
        assert!(
            !player.mutate_with_update_context(|context| context.input.is_key_down(KeyCode::W)),
            "a menu opened outside the player releases held keys too"
        );
        assert!(
            input
                .handle_event(
                    &mut player,
                    PlayerEvent::MouseDown {
                        x: 40.0,
                        y: 60.0,
                        button: MouseButton::Left,
                        index: Some(0),
                    }
                )
                .is_none()
        );
    }
}
