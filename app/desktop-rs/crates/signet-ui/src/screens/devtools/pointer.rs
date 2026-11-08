//! Pointer capture for drag handles. GPUI delivers mouse events to the hovered element only; a handle held down must keep
//! receiving them once the pointer leaves it, so while a drag runs the view renders [`track_pointer`], which registers
//! window-wide listeners for the frame (the equivalent of `setPointerCapture`), plus Escape to cancel.

use gpui::{App, Context, DispatchPhase, Entity, IntoElement, KeyDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Styled, Window, canvas};

pub type PointerFn<V> = fn(&mut V, Point<Pixels>, &mut Window, &mut Context<V>);

/// Invisible element that routes the pointer's moves and release, and Escape, to `view` while it is rendered.
pub fn track_pointer<V: 'static>(view: Entity<V>, on_move: PointerFn<V>, on_up: PointerFn<V>, on_escape: fn(&mut V, &mut Window, &mut Context<V>)) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |_, _, window: &mut Window, _cx: &mut App| {
            let v = view.clone();
            window.on_mouse_event(move |ev: &MouseMoveEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture {
                    v.update(cx, |this, cx| on_move(this, ev.position, window, cx));
                }
            });
            let v = view.clone();
            window.on_mouse_event(move |ev: &MouseUpEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture {
                    v.update(cx, |this, cx| on_up(this, ev.position, window, cx));
                }
            });
            let v = view.clone();
            window.on_key_event(move |ev: &KeyDownEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture && ev.keystroke.key == "escape" {
                    cx.stop_propagation();
                    v.update(cx, |this, cx| on_escape(this, window, cx));
                }
            });
        },
    )
    .absolute()
    .size_full()
}
