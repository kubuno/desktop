//! `08-views` — TreeView, ListView.

use kubuno_controls::views::{ColumnHeader, ListView, ListViewItem, TreeNode, TreeView, View};

use crate::sheet::{group, kid, Group, Sheet};

pub fn build() -> Sheet {
    Sheet::new(vec![tree(), details(), list()])
}

fn tree() -> Group {
    let mut t = TreeView::new();
    t.show_lines = true;
    t.show_root_lines = true;
    t.nodes = vec![TreeNode::new("Instance")
        .child(
            TreeNode::new("Kubuno")
                .child(TreeNode::new("Support N1"))
                .child(TreeNode::new("Équipe support"))
                .expanded(),
        )
        .child(TreeNode::new("Invités"))
        .expanded()];
    // `SelectedNode = a` in the reference: the second row, « Kubuno ».
    t.selected_path = Some(vec![0, 0]);
    group("TreeView — lines / selection", 300.0, vec![kid(t).size(260.0, 160.0)])
}

fn details() -> Group {
    let mut l = ListView::new();
    l.view = View::Details;
    l.full_row_select = true;
    l.grid_lines = true;
    l.columns = vec![
        ColumnHeader::new("Nom", 140),
        ColumnHeader::new("Rôle", 100),
        ColumnHeader::new("Quota", 100),
    ];
    let mut admin = ListViewItem::new("Admin").with_sub("admin").with_sub("10 Go");
    admin.selected = true;
    l.items = vec![
        admin,
        ListViewItem::new("Alice").with_sub("user").with_sub("5 Go"),
        ListViewItem::new("Bob").with_sub("user").with_sub("5 Go"),
    ];
    group("ListView — View=Details", 420.0, vec![kid(l).size(380.0, 160.0)])
}

fn list() -> Group {
    let mut l = ListView::new();
    l.view = View::List;
    l.items = (1..=8).map(|i| ListViewItem::new(format!("élément {i}"))).collect();
    group("ListView — View=List", 420.0, vec![kid(l).size(380.0, 110.0)])
}
