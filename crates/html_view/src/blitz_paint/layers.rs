use anyrender::{Filter, PaintScene};
use kurbo::{Affine, Shape};
use peniko::{BlendMode, Fill};
use std::{cell::Cell, sync::Arc};

const LAYER_LIMIT: u32 = 1024;

#[derive(Default)]
pub(crate) struct LayerManager {
    layers_used: Cell<u32>,
    layer_depth: Cell<u32>,
    layers_wanted: Cell<u32>,

    #[allow(unused)] // Only used for debugging. Enabled as required.
    layer_depth_used: Cell<u32>,
}

impl LayerManager {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn maybe_with_layer<F: FnOnce(&mut crate::display_list::DisplayList)>(
        &self,
        scene: &mut crate::display_list::DisplayList,
        condition: bool,
        blend_mode: BlendMode,
        opacity: f32,
        transform: Affine,
        shape: &impl Shape,
        filter: Option<Arc<Filter>>,
        backdrop_filter: Option<Arc<Filter>>,
        paint_layer: F,
    ) {
        let layer_used = self.maybe_push_layer(
            scene,
            condition,
            blend_mode,
            opacity,
            transform,
            shape,
            filter,
            backdrop_filter,
        );
        paint_layer(scene);
        self.maybe_pop_layer(scene, layer_used);
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn maybe_push_layer(
        &self,
        scene: &mut crate::display_list::DisplayList,
        condition: bool,
        blend_mode: BlendMode,
        opacity: f32,
        transform: Affine,
        shape: &impl Shape,
        filter: Option<Arc<Filter>>,
        backdrop_filter: Option<Arc<Filter>>,
    ) -> bool {
        if !condition {
            return false;
        }
        self.layers_wanted.update(|x| x + 1);

        // Check if clips are above limit
        let layers_available = self.layers_used.get() <= LAYER_LIMIT;
        if !layers_available {
            return false;
        }

        // Actually push the layer
        if blend_mode == BlendMode::default()
            && opacity == 1.0
            && filter.is_none()
            && backdrop_filter.is_none()
        {
            scene.push_clip_layer(Fill::NonZero, transform, shape);
        } else {
            scene.push_layer(
                Fill::NonZero,
                blend_mode,
                opacity,
                transform,
                shape,
                filter,
                backdrop_filter,
            );
        };

        // Update accounting
        self.layers_used.update(|x| x + 1);
        self.layer_depth.update(|x| x + 1);
        self.layer_depth.update(|x| x.max(self.layer_depth.get()));

        true
    }

    pub(crate) fn maybe_pop_layer(
        &self,
        scene: &mut crate::display_list::DisplayList,
        condition: bool,
    ) {
        if condition {
            scene.pop_layer();
            self.layer_depth.update(|x| x - 1);
        }
    }
}

#[cfg(test)]
mod tests {
    // Adapted from upstream blitz-paint: our display list records layers as
    // `Command::PushLayer { clip, alpha }` (GPUI has no blend modes).
    use super::*;
    use crate::display_list::{Command, DisplayList};
    use kurbo::Rect;
    use peniko::{Compose, Mix};

    #[test]
    fn blend_layers_push_and_pop() {
        for blend in [
            Mix::Multiply.into(),
            BlendMode::new(Mix::Normal, Compose::PlusLighter),
        ] {
            let manager = LayerManager::default();
            let mut scene = DisplayList::new();
            manager.maybe_with_layer(
                &mut scene,
                true,
                blend,
                1.0,
                Affine::IDENTITY,
                &Rect::ZERO,
                None,
                None,
                |_| {},
            );
            assert!(matches!(scene.commands[0], Command::PushLayer { .. }));
            assert!(matches!(scene.commands[1], Command::Pop));
        }
    }

    #[test]
    fn isolated_layers_carry_their_opacity() {
        for (opacity, isolated) in [(1.0, false), (1.0 - f32::EPSILON, true)] {
            let manager = LayerManager::default();
            let mut scene = DisplayList::new();
            manager.maybe_with_layer(
                &mut scene,
                true,
                BlendMode::default(),
                opacity,
                Affine::IDENTITY,
                &Rect::ZERO,
                None,
                None,
                |_| {},
            );
            assert!(
                matches!(scene.commands[0], Command::PushLayer { alpha, .. } if (alpha < 1.0) == isolated)
            );
            assert!(matches!(scene.commands[1], Command::Pop));
        }
    }

    #[test]
    fn disabled_or_exhausted_layers_do_not_pop() {
        let manager = LayerManager::default();
        let mut scene = DisplayList::new();
        for condition in [false, true] {
            manager.layers_used.set(LAYER_LIMIT + 1);
            manager.maybe_with_layer(
                &mut scene,
                condition,
                Mix::Multiply.into(),
                1.0,
                Affine::IDENTITY,
                &Rect::ZERO,
                None,
                None,
                |_| {},
            );
        }
        assert!(scene.commands.is_empty());
    }
}
