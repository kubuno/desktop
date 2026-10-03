//! `03-listcontrol` — ComboBox, ListBox, CheckedListBox.

use kubuno_controls::lists::{CheckedListBox, ComboBox, ComboBoxStyle, ListBox, SelectionMode};
use kubuno_controls::CheckState;

use crate::sheet::{group, kid, Group, Sheet};

const ITEMS: [&str; 6] = ["Alpha", "Beta", "Gamma", "Delta", "Epsilon", "Zeta"];

pub fn build() -> Sheet {
    Sheet::new(vec![combos(), list_boxes(), checked()])
}

fn combos() -> Group {
    let combo = |style: ComboBoxStyle, selected: i32| {
        let mut cb = ComboBox::new();
        cb.drop_down_style = style;
        for item in ITEMS {
            cb.add_item(item);
        }
        cb.set_selected_index(selected);
        cb
    };
    group(
        "ComboBox — DropDownStyle",
        300.0,
        vec![
            kid(combo(ComboBoxStyle::DropDown, 0)).w(240.0),
            kid(combo(ComboBoxStyle::DropDownList, 1)).w(240.0),
            // `Simple` keeps the list open under the edit field, so it is the one
            // style with a designed height.
            kid(combo(ComboBoxStyle::Simple, 2)).size(240.0, 80.0),
        ],
    )
}

fn list_boxes() -> Group {
    let mut single = ListBox::new();
    for item in ITEMS {
        single.add_item(item);
    }
    single.set_selected_index(1);

    // The mode has to be widened before more than one index can be held: in
    // `SelectionMode::One` the setter replaces rather than adds.
    let mut multi = ListBox::new();
    for item in ITEMS {
        multi.add_item(item);
    }
    multi.set_selection_mode(SelectionMode::MultiExtended);
    for i in [0, 2, 3] {
        multi.set_selected(i, true);
    }

    group(
        "ListBox — SelectionMode",
        300.0,
        vec![kid(single).size(240.0, 110.0), kid(multi).size(240.0, 110.0)],
    )
}

fn checked() -> Group {
    let mut clb = CheckedListBox::new();
    clb.check_on_click = true;
    for item in ITEMS {
        clb.add_item(item, CheckState::Unchecked);
    }
    clb.set_item_checked(0, true);
    clb.set_item_checked(2, true);
    clb.set_item_check_state(3, CheckState::Indeterminate);
    group("CheckedListBox", 300.0, vec![kid(clb).size(240.0, 120.0)])
}
