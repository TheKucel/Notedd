//! Nested lists: reading, writing, Tab / Shift+Tab, Enter and Backspace.

use crate::model::*;
use crate::{markdown, Core};

fn lv(c: &Core) -> Vec<u8> { levels(&c.doc.blocks) }
fn at(c: &mut Core, block: usize, off: usize) { c.set_selection(Pos::new(block, off), Pos::new(block, off)); }

#[test]
fn indented_items_nest_by_their_indent() {
    let c = Core::from_markdown("T\n- a\n  - b\n    - c\n- d");
    assert_eq!(lv(&c), [0, 0, 1, 2, 0]);
    // four spaces, tabs and mixed widths read the same way
    let c = Core::from_markdown("T\n1. a\n    1. b\n\t\t1. c\n2. d");
    assert_eq!(lv(&c), [0, 0, 1, 2, 0]);
    // the first item sets the base; a deeper start is still the top level
    let c = Core::from_markdown("T\n    - a\n    - b\n- c");
    assert_eq!(lv(&c), [0, 0, 0, 0]);
    // a paragraph or a blank line ends the list
    let c = Core::from_markdown("T\n- a\n    - b\ntext\n    - c");
    assert_eq!(lv(&c), [0, 0, 1, 0, 0]);
    // nesting stops at the deepest level
    let c = Core::from_markdown("T\n- a\n  - b\n    - c\n      - d\n        - e");
    assert_eq!(lv(&c), [0, 0, 1, 2, 3, 3]);
}

#[test]
fn nested_lists_are_written_with_four_spaces_and_count_per_level() {
    let src = "T\n1. a\n    1. b\n    2. c\n        - x\n2. d\n    1. e";
    let c = Core::from_markdown(src);
    assert_eq!(c.markdown(), src);
    assert_eq!(numbering(&c.doc.blocks), [0, 1, 1, 2, 0, 2, 1]);
    // a bullet between numbers restarts the count at its level
    let c = Core::from_markdown("T\n1. a\n- b\n1. c");
    assert_eq!(c.markdown(), "T\n1. a\n- b\n1. c");
}

#[test]
fn a_paragraph_that_looks_like_a_nested_item_stays_a_paragraph() {
    let mut c = Core::from_markdown("T
");
    at(&mut c, 1, 0);
    c.insert_raw("    - not a list");
    let back = markdown::parse(&c.markdown());
    assert_eq!(back.blocks[1].kind, Kind::Para);
    assert_eq!(back.blocks[1].text(), "    - not a list");
}

#[test]
fn tab_nests_an_item_under_the_one_above_and_takes_its_children_along() {
    let mut c = Core::from_markdown("T\n- a\n- b\n    - c\n- d");
    // the first item has nothing to nest under
    at(&mut c, 1, 0);
    assert!(!c.change_indent(true));
    assert_eq!(lv(&c), [0, 0, 0, 1, 0]);
    // b moves under a, and c stays under b
    at(&mut c, 2, 1);
    assert!(c.change_indent(true));
    assert_eq!(lv(&c), [0, 0, 1, 2, 0]);
    // not more than one level below the item above
    assert!(!c.change_indent(true));
    // Shift+Tab brings them back
    assert!(c.change_indent(false));
    assert_eq!(lv(&c), [0, 0, 0, 1, 0]);
    // the top level has nothing to leave
    at(&mut c, 1, 0);
    assert!(!c.change_indent(false));
    // one undo step per press
    c.undo();
    assert_eq!(lv(&c), [0, 0, 1, 2, 0]);
}

#[test]
fn tab_moves_every_selected_item() {
    let mut c = Core::from_markdown("T\n- a\n- b\n- c\n- d");
    c.set_selection(Pos::new(2, 0), Pos::new(3, 1));
    assert!(c.change_indent(true));
    assert_eq!(lv(&c), [0, 0, 1, 1, 0]);
    c.set_selection(Pos::new(2, 0), Pos::new(4, 0));
    assert!(c.change_indent(false));
    assert_eq!(lv(&c), [0, 0, 0, 0, 0]);
}

#[test]
fn tab_outside_a_list_does_nothing() {
    let mut c = Core::from_markdown("T\nplain\n> quote");
    at(&mut c, 1, 2);
    assert!(!c.change_indent(true));
    at(&mut c, 2, 0);
    assert!(!c.change_indent(true));
    assert_eq!(c.markdown(), "T\nplain\n> quote");
}

#[test]
fn nesting_has_a_floor_and_a_ceiling() {
    let mut c = Core::from_markdown("T\n- a\n- b\n- c\n- d\n- e");
    for i in 2..=5 { at(&mut c, i, 0); while c.change_indent(true) {} }
    assert_eq!(lv(&c), [0, 0, 1, 2, 3, 3]);
}

#[test]
fn enter_on_an_empty_nested_item_steps_out_one_level() {
    let mut c = Core::from_markdown("T\n- a\n    - b\n    - ");
    at(&mut c, 3, 0);
    c.enter();
    assert_eq!(lv(&c), [0, 0, 1, 0]);
    assert_eq!(c.doc.blocks[3].kind, Kind::Bullet);
    c.enter();
    assert_eq!(c.doc.blocks[3].kind, Kind::Para);
    // a new item starts at the level of the one it follows
    let mut c = Core::from_markdown("T\n- a\n    - b");
    at(&mut c, 2, 1);
    c.enter();
    assert_eq!(lv(&c), [0, 0, 1, 1]);
}

#[test]
fn backspace_at_the_start_of_a_nested_item_steps_out_first() {
    let mut c = Core::from_markdown("T\n- a\n    - b");
    at(&mut c, 2, 0);
    c.backspace();
    assert_eq!((lv(&c), c.doc.blocks[2].kind.clone()), (vec![0, 0, 0], Kind::Bullet));
    c.backspace();
    assert_eq!(c.doc.blocks[2].kind, Kind::Para);
}

#[test]
fn items_left_without_a_parent_move_up() {
    // the parent turns into a paragraph: its children cannot hang from it
    let mut c = Core::from_markdown("T\n- a\n    - b\n        - c");
    at(&mut c, 1, 0);
    c.backspace();
    assert_eq!(c.doc.blocks[1].kind, Kind::Para);
    assert_eq!(lv(&c), [0, 0, 0, 1]);
    // deleting an item keeps the rest in shape
    let mut c = Core::from_markdown("T\n- a\n    - b\n        - c");
    c.set_selection(Pos::new(1, 0), Pos::new(2, 1));
    c.delete_selection();
    assert!(c.doc.check().is_ok());
}

#[test]
fn a_copied_nested_selection_starts_at_the_top_level() {
    let mut c = Core::from_markdown("T
- a
    - b
        - c
    - d");
    c.set_selection(Pos::new(2, 0), Pos::new(4, 1));
    assert_eq!(c.selection_markdown(), "- b
    - c
- d");
}

#[test]
fn undo_restores_the_levels() {
    let mut c = Core::from_markdown("T\n- a\n- b");
    let before = c.doc.clone();
    at(&mut c, 2, 0);
    assert!(c.change_indent(true));
    assert_ne!(c.doc, before);
    c.undo();
    assert_eq!(c.doc, before);
    c.redo();
    assert_eq!(lv(&c), [0, 0, 1]);
}
