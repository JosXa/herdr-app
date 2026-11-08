//! A record of each drawn frame's elements and text, for tools that drive an app from outside it.
//!
//! Recording is off until [`Window::set_driver_enabled`](crate::Window::set_driver_enabled) turns it on.

use crate::{Bounds, ContentMask, FocusId, Pixels, SharedString};
use std::panic::Location;

/// What an element responds to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DriverFlags {
    /// Has a click listener.
    pub click: bool,
    /// Has a mouse down, up, move or pressure listener.
    pub mouse: bool,
    /// Has a key or action listener.
    pub keys: bool,
    /// Has a hover style or hover listener.
    pub hover: bool,
    /// Tracks a focus handle.
    pub focusable: bool,
    /// Its focus handle is focused.
    pub focused: bool,
    /// Scrolls or listens for scroll wheel events.
    pub scroll: bool,
    /// Can be dragged.
    pub drag: bool,
    /// Accepts drops.
    pub drop: bool,
    /// Shows a tooltip on hover.
    pub tooltip: bool,
    /// Its hitbox stops the mouse from reaching what is behind it.
    pub blocks: bool,
}

/// One element drawn in the last frame.
#[derive(Clone, Debug)]
pub struct DriverNode {
    /// Index of the parent node in the same frame.
    pub parent: Option<usize>,
    /// Layout bounds in window coordinates.
    pub bounds: Bounds<Pixels>,
    /// The part of `bounds` inside its content mask. Empty when clipped away.
    pub visible_bounds: Bounds<Pixels>,
    /// The element's own id, if it has one.
    pub element_id: Option<SharedString>,
    /// The Rust type of the element.
    pub type_name: &'static str,
    /// Where the element was constructed.
    pub source: Option<&'static Location<'static>>,
    /// The accessibility role the element declares.
    pub role: Option<accesskit::Role>,
    /// The accessible name the element gives itself, such as an icon button's label.
    pub label: Option<SharedString>,
    /// The accessible value the element reports, such as a field's text.
    pub value: Option<SharedString>,
    /// The element reports itself disabled.
    pub disabled: bool,
    /// The element's checked state, when it has one.
    pub toggled: Option<accesskit::Toggled>,
    /// What the element responds to.
    pub flags: DriverFlags,
    /// The primary entry of the key context the element sets, such as `Editor`.
    pub key_context: Option<SharedString>,
    /// The focus handle the element tracks.
    pub focus: Option<FocusId>,
    /// The lines of text the element painted itself, in paint order.
    pub texts: Vec<DriverText>,
}

/// One line of text painted in the last frame.
#[derive(Clone, Debug)]
pub struct DriverText {
    /// The line's bounds in window coordinates, before alignment.
    pub bounds: Bounds<Pixels>,
    /// The part of `bounds` inside its content mask.
    pub visible_bounds: Bounds<Pixels>,
    /// The painted string.
    pub text: SharedString,
}

pub(crate) fn visible(bounds: Bounds<Pixels>, mask: ContentMask<Pixels>) -> Bounds<Pixels> {
    bounds.intersect(&mask.bounds)
}

pub(crate) fn reparent(
    nodes: &[DriverNode],
    old_start: usize,
    new_start: usize,
    outer_parent: Option<usize>,
) -> impl Iterator<Item = DriverNode> + '_ {
    nodes.iter().map(move |node| {
        let mut node = node.clone();
        node.parent = match node.parent {
            Some(parent) if parent >= old_start => Some(parent - old_start + new_start),
            _ => outer_parent,
        };
        node
    })
}
