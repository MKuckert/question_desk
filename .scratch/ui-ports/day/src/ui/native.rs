//! The few controls Day has no piece for, made from Day pieces plus a native tweak.
//!
//! Day's `toggle` is an `NSSwitch` on macOS and its radio group (`picker(..).inline()`) takes
//! plain titles only, so an Option is a Day `button` whose `NSButton` is switched to the
//! checkbox or radio type (docs/tweaks.md "Per-toolkit access"). Elsewhere the Option falls
//! back to Day's `toggle` beside a label.

use day::prelude::*;
use day::reactive::{bind, untrack};

/// How far an Option's description sits in, so it lines up with the title after the mark.
pub(crate) const CHOICE_INDENT: f64 = 20.0;

/// A native checkbox (`multi`) or radio button titled `title`. The desk decides what is
/// selected: a click only calls `toggle`, and `selected` drives the mark.
#[cfg(target_os = "macos")]
pub(crate) fn choice(
    title: String,
    multi: bool,
    selected: impl Fn() -> bool + 'static,
    toggle: impl Fn() + 'static,
) -> impl Piece {
    use day_toolkit_appkit::{appkit, with_native};
    use objc2_app_kit::{NSButton, NSButtonType};

    fn set_on(view: &objc2_app_kit::NSView, on: bool) {
        if let Some(button) = view.downcast_ref::<NSButton>() {
            button.setState(isize::from(on));
        }
    }

    let selected = std::rc::Rc::new(selected);
    let initial = untrack(|| selected());
    let node = NativeRef::new();
    let watched = node.clone();
    let desk_side = selected.clone();
    bind(
        move || (watched.node(), desk_side()),
        move |&(node, on): &(Option<day::RNode>, bool)| {
            if let Some(node) = node {
                with_native(node, |view, _, _| set_on(view, on));
            }
        },
    );
    let clicked = node.clone();
    button(title)
        .action(move || {
            toggle();
            // After a press Day puts back the button state it last set itself (none, so
            // off), so the mark is set again once it is done.
            let node = clicked.clone();
            let selected = selected.clone();
            day::task(async move {
                day::sleep(0).await;
                if let Some(node) = node.node() {
                    let on = untrack(|| selected());
                    with_native(node, |view, _, _| set_on(view, on));
                }
            });
        })
        .native_ref(&node)
        .tweak(appkit(move |view, _, _| {
            if let Some(button) = view.downcast_ref::<NSButton>() {
                button.setButtonType(if multi {
                    NSButtonType::Switch
                } else {
                    NSButtonType::Radio
                });
                button.setBordered(false);
            }
            set_on(view, initial);
        }))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn choice(
    title: String,
    _multi: bool,
    selected: impl Fn() -> bool + 'static,
    on_click: impl Fn() + 'static,
) -> impl Piece {
    use day::reactive::Signal;
    let selected = std::rc::Rc::new(selected);
    let on = Signal::new(untrack(|| selected()));
    let desk_side = selected.clone();
    bind(move || desk_side(), move |&v: &bool| on.set(v));
    bind(
        move || on.get(),
        move |&v: &bool| {
            if v != untrack(|| selected()) {
                on_click();
            }
        },
    );
    row((toggle(on), label(title))).spacing(Space::S)
}

/// `piece` without the platform's focus ring: for the page's invisible key catcher, which
/// would otherwise outline the whole page while it has the keyboard.
#[cfg(target_os = "macos")]
pub(crate) fn quiet_focus<P: Piece>(piece: P) -> impl Piece {
    use day_toolkit_appkit::appkit;
    piece.tweak(appkit(|view, _, _| {
        view.setFocusRingType(objc2_app_kit::NSFocusRingType::None);
    }))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn quiet_focus<P: Piece>(piece: P) -> impl Piece {
    piece
}

/// The Note's look on macOS: Day draws its text area borderless and transparent, which leaves
/// nothing to click into on an empty page. Give the `NSScrollView` the standard bezel and the
/// `NSTextView` the text background, as in Mail or Notes' sheets, and let the placeholder use
/// the full width (Day pins it at 320 pt, which cuts the longer hints).
#[cfg(target_os = "macos")]
pub(crate) fn bezeled_text_area<P: Piece>(piece: P) -> impl Piece {
    use day_toolkit_appkit::appkit;
    use objc2_app_kit::{NSBorderType, NSColor, NSScrollView, NSTextField, NSTextView};
    piece.tweak(appkit(|view, _, _| {
        let Some(scroll) = view.downcast_ref::<NSScrollView>() else {
            return;
        };
        scroll.setBorderType(NSBorderType::BezelBorder);
        let Some(document) = scroll.documentView() else {
            return;
        };
        if let Some(text) = document.downcast_ref::<NSTextView>() {
            text.setDrawsBackground(true);
            text.setBackgroundColor(&NSColor::textBackgroundColor());
        }
        for sub in document.subviews().iter() {
            if let Some(placeholder) = sub.downcast_ref::<NSTextField>() {
                let mut frame = placeholder.frame();
                frame.size.width = 2000.0;
                placeholder.setFrame(frame);
            }
        }
    }))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn bezeled_text_area<P: Piece>(piece: P) -> impl Piece {
    piece
}

/// Marks the button titled `title` in the alert sheet that is about to show as destructive
/// (red title, macOS 11+). Day's `Alert` has the role but does not tell AppKit.
#[cfg(target_os = "macos")]
pub(crate) fn mark_destructive(title: &'static str) {
    use objc2_app_kit::{NSApplication, NSButton, NSView};
    fn visit(view: &NSView, title: &str) {
        if let Some(button) = view.downcast_ref::<NSButton>()
            && button.title().to_string() == title
        {
            button.setHasDestructiveAction(true);
        }
        for sub in view.subviews().iter() {
            visit(&sub, title);
        }
    }
    day::task(async move {
        day::sleep(0).await;
        let Some(mtm) = objc2::MainThreadMarker::new() else {
            return;
        };
        for window in NSApplication::sharedApplication(mtm).windows().iter() {
            if let Some(content) = window.attachedSheet().and_then(|s| s.contentView()) {
                visit(&content, title);
            }
        }
    });
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn mark_destructive(_title: &'static str) {}

/// Keeps the placeholder of the text area behind `node` on `hint` (Day sets it once).
#[cfg(target_os = "macos")]
pub(crate) fn follow_placeholder(node: &NativeRef, hint: impl Fn() -> &'static str + 'static) {
    use day_toolkit_appkit::with_native;
    use objc2_app_kit::{NSScrollView, NSTextField};
    use objc2_foundation::NSString;
    let watched = node.clone();
    bind(
        move || (watched.node(), hint()),
        |&(node, hint): &(Option<day::RNode>, &'static str)| {
            let Some(node) = node else { return };
            with_native(node, |view, _, _| {
                let Some(document) = view
                    .downcast_ref::<NSScrollView>()
                    .and_then(|s| s.documentView())
                else {
                    return;
                };
                for sub in document.subviews().iter() {
                    if let Some(placeholder) = sub.downcast_ref::<NSTextField>() {
                        placeholder.setStringValue(&NSString::from_str(hint));
                    }
                }
            });
        },
    );
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn follow_placeholder(_node: &NativeRef, _hint: impl Fn() -> &'static str + 'static) {}

/// Routes the main window's close button to `on_close`.
///
/// Day's `WindowHandle::on_close_request` does not reach the primary window on AppKit (0.7.10:
/// its window delegate asks about `WINDOW_NODE`, while the handle registered the handler under
/// the window's own node), so the button gets a target of ours instead.
#[cfg(target_os = "macos")]
pub(crate) fn intercept_close_button(on_close: fn()) {
    use objc2::{MainThreadOnly, define_class, msg_send, rc::Retained, runtime::AnyObject, sel};
    use objc2_app_kit::{NSApplication, NSWindowButton};
    use objc2_foundation::NSObject;

    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "QDCloseButtonTarget"]
        struct CloseTarget;

        impl CloseTarget {
            #[unsafe(method(closeClicked:))]
            fn close_clicked(&self, _sender: Option<&AnyObject>) {
                if let Some(f) = ON_CLOSE.with(std::cell::Cell::get) {
                    f();
                }
            }
        }
    );

    thread_local! {
        static ON_CLOSE: std::cell::Cell<Option<fn()>> = const { std::cell::Cell::new(None) };
        // NSControl targets are weak: keep ours alive.
        static TARGET: std::cell::RefCell<Option<Retained<CloseTarget>>> = const { std::cell::RefCell::new(None) };
    }

    let Some(mtm) = objc2::MainThreadMarker::new() else { return };
    ON_CLOSE.with(|c| c.set(Some(on_close)));
    let target: Retained<CloseTarget> = unsafe { msg_send![CloseTarget::alloc(mtm), init] };
    for window in NSApplication::sharedApplication(mtm).windows().iter() {
        if let Some(button) = window.standardWindowButton(NSWindowButton::CloseButton) {
            unsafe {
                button.setTarget(Some(&target));
                button.setAction(Some(sel!(closeClicked:)));
            }
        }
    }
    TARGET.with(|t| *t.borrow_mut() = Some(target));
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn intercept_close_button(_on_close: fn()) {}
