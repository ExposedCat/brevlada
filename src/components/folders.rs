use super::{
    column, horizontal, label,
    sidebar::{Selection, folder_icon},
};
use crate::theme;
use adw::prelude::*;
use std::{collections::BTreeMap, rc::Rc};

#[derive(Default)]
struct FolderNode {
    path: String,
    children: BTreeMap<String, FolderNode>,
}

pub fn populate(
    container: &gtk::Box,
    folders: Vec<String>,
    selection: Selection,
    select: impl Fn(String) + 'static,
) {
    let mut root = FolderNode::default();
    for folder in folders {
        if folder.eq_ignore_ascii_case("INBOX") {
            continue;
        }
        let mut node = &mut root;
        let mut path = String::new();
        for segment in folder.split('/') {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(segment);
            node = node.children.entry(segment.to_owned()).or_default();
            node.path = path.clone();
        }
    }
    append(container, root, selection, Rc::new(select), 1);
}

fn append(
    container: &gtk::Box,
    node: FolderNode,
    selection: Selection,
    select: Rc<dyn Fn(String)>,
    depth: i32,
) {
    for (name, child) in node.children {
        let row = column("folder-item");
        let content = horizontal("folder-content", theme::ROW_VERTICAL_GAP);
        let has_children = !child.children.is_empty();
        let icon = gtk::Image::from_icon_name(if has_children {
            "pan-end-symbolic"
        } else {
            folder_icon(&child.path)
        });
        content.append(&icon);
        content.append(&label(&name, "folder-text"));
        let button = gtk::Button::builder()
            .child(&content)
            .hexpand(true)
            .css_classes(["flat", "folder-button"])
            .margin_start(theme::INDENT * depth)
            .build();
        row.append(&button);
        let children = column("folder-children");
        children.set_visible(false);
        append(
            &children,
            FolderNode {
                path: String::new(),
                children: child.children,
            },
            selection.clone(),
            select.clone(),
            depth + 1,
        );
        row.append(&children);
        let selected = selection.clone();
        let callback = select.clone();
        button.connect_clicked(move |button| {
            if has_children {
                let expanded = !children.is_visible();
                children.set_visible(expanded);
                icon.set_icon_name(Some(if expanded {
                    "pan-down-symbolic"
                } else {
                    "pan-end-symbolic"
                }));
            } else {
                selected.activate(button);
                callback(child.path.clone());
            }
        });
        container.append(&row);
    }
}
