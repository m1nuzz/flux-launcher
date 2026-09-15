use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use windui::app::App;
use windui::event::{Key, KeyEvent};
use windui::prelude::*;
use windui::signal::{signal, Signal};

use super::theme_text::launcher_theme;
use super::ui_constants::{RECYCLE_CONFIRM_WINDOW_HEIGHT, RECYCLE_CONFIRM_WINDOW_WIDTH};

const RESULT_EMPTY: i32 = 0;
const RESULT_CANCEL: i32 = 1;
const CANCEL_SLOT: usize = 0;
const EMPTY_SLOT: usize = 1;
const MARKER: char = '\u{25B8}';

fn label_for(slot: usize, selected: usize, base: &str) -> String {
    if slot == selected {
        format!("{MARKER} {base}")
    } else {
        base.to_string()
    }
}

fn refresh_labels(cancel_text: Signal<String>, empty_text: Signal<String>, selected: usize) {
    cancel_text.set(label_for(CANCEL_SLOT, selected, "Cancel"));
    empty_text.set(label_for(EMPTY_SLOT, selected, "Empty Recycle Bin"));
}

/// Entry point for the standalone `--empty-recycle-confirm` child process.
///
/// It owns a real centered top-level window that replaces the earlier in-launcher
/// confirmation overlay, which was laid out below the compact search strip and could
/// end up off the bottom of the window where it could not be reached. The launcher hides
/// itself while this process is open, then empties the Recycle Bin (or reports a cancel)
/// from the child's exit code: 0 = confirmed, anything else = cancelled or dismissed.
pub(crate) fn run() -> ! {
    let selection = signal(EMPTY_SLOT);
    let cancel_text = signal(String::new());
    let empty_text = signal(String::new());
    refresh_labels(cancel_text, empty_text, EMPTY_SLOT);

    let result = Rc::new(Cell::new(RESULT_CANCEL));
    let close = Rc::new(Cell::new(false));

    let result_for_cancel = Rc::clone(&result);
    let close_for_cancel = Rc::clone(&close);
    let cancel_button = Element::button(cancel_text)
        .neutral()
        .outline_soft()
        .on_click(move |_| {
            result_for_cancel.set(RESULT_CANCEL);
            close_for_cancel.set(true);
        });

    let result_for_empty = Rc::clone(&result);
    let close_for_empty = Rc::clone(&close);
    let empty_button = Element::button(empty_text).danger().on_click(move |_| {
        result_for_empty.set(RESULT_EMPTY);
        close_for_empty.set(true);
    });

    let content = Element::col()
        .fill()
        .padding(20)
        .spacing(12)
        .child(
            Element::label("Empty Recycle Bin")
                .font_size(18.0)
                .font_weight(700)
                .fg(Color::WHITE),
        )
        .child(
            Element::label("This permanently deletes all items in the Recycle Bin.")
                .font_size(13.0)
                .fg(Color::rgba(245, 248, 255, 245)),
        )
        .child(
            Element::label("This action cannot be undone.")
                .font_size(12.0)
                .fg(Color::rgba(255, 190, 190, 235)),
        )
        .child(
            Element::row()
                .width_match()
                .spacing(8)
                .child(Element::flex_spacer())
                .child(cancel_button)
                .child(empty_button),
        );

    let result_for_keys = Rc::clone(&result);
    let close_for_keys = Rc::clone(&close);

    let app = App::new(
        "Empty Recycle Bin",
        RECYCLE_CONFIRM_WINDOW_WIDTH,
        RECYCLE_CONFIRM_WINDOW_HEIGHT,
    )
    .centered()
    .theme(launcher_theme())
    .content(content)
    .on_key(move |event: KeyEvent| {
        if !event.pressed || event.ctrl {
            return false;
        }
        let selected = selection.get();
        match event.key {
            Key::Enter | Key::Space => {
                result_for_keys.set(if selected == EMPTY_SLOT {
                    RESULT_EMPTY
                } else {
                    RESULT_CANCEL
                });
                close_for_keys.set(true);
                true
            }
            Key::Escape => {
                result_for_keys.set(RESULT_CANCEL);
                close_for_keys.set(true);
                true
            }
            Key::Left | Key::Up => {
                if selected != CANCEL_SLOT {
                    selection.set(CANCEL_SLOT);
                    refresh_labels(cancel_text, empty_text, CANCEL_SLOT);
                }
                true
            }
            Key::Right | Key::Down => {
                if selected != EMPTY_SLOT {
                    selection.set(EMPTY_SLOT);
                    refresh_labels(cancel_text, empty_text, EMPTY_SLOT);
                }
                true
            }
            Key::Tab => {
                let next = if selected == EMPTY_SLOT {
                    CANCEL_SLOT
                } else {
                    EMPTY_SLOT
                };
                selection.set(next);
                refresh_labels(cancel_text, empty_text, next);
                true
            }
            _ => false,
        }
    })
    .on_interval(Duration::from_millis(16), move |ctx| {
        if close.get() {
            ctx.request_close();
        }
    });

    app.run();
    std::process::exit(result.get());
}
