use crate::{
    models::{Account, Message},
    theme,
};
use adw::prelude::*;

#[derive(Clone)]
pub struct FileCard {
    pub widget: gtk::Box,
    pub status: gtk::Label,
    pub actions: gtk::Box,
    header: gtk::Box,
    footer: gtk::Box,
}

impl FileCard {
    pub fn new(name: &str, size: Option<u64>) -> Self {
        let (content_type, _) =
            gtk::gio::content_type_guess(Some(std::path::Path::new(name)), None);
        let widget = super::column("attachment-tile");
        widget.add_css_class("card");
        widget.set_width_request(theme::ATTACHMENT_TILE_WIDTH);
        let header = super::horizontal("attachment-tile-header", theme::SMALL_SPACING);
        let icon = gtk::Image::from_gicon(&gtk::gio::content_type_get_icon(&content_type));
        icon.set_pixel_size(theme::AVATAR_SIZE);
        let title = super::label(name, "heading");
        title.set_wrap(true);
        title.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        title.set_lines(2);
        title.set_width_chars(theme::ATTACHMENT_NAME_MIN_CHARS);
        title.set_max_width_chars(theme::ATTACHMENT_NAME_MAX_CHARS);
        title.set_halign(gtk::Align::Fill);
        title.set_tooltip_text(Some(name));
        header.append(&icon);
        header.append(&title);
        widget.append(&header);
        let footer = super::horizontal("attachment-tile-footer", theme::SMALL_SPACING);
        let subtitle = size
            .map(|size| gtk::glib::format_size(size).to_string())
            .unwrap_or_else(|| gtk::gio::content_type_get_description(&content_type).to_string());
        let status = super::label(&subtitle, "dim-label");
        status.set_halign(gtk::Align::Fill);
        status.set_max_width_chars(theme::ATTACHMENT_NAME_MAX_CHARS);
        let actions = super::horizontal("attachment-actions", theme::SMALL_SPACING);
        footer.append(&status);
        footer.append(&actions);
        widget.append(&footer);
        Self {
            widget,
            status,
            actions,
            header,
            footer,
        }
    }

    pub fn open_button(&self, name: &str) -> gtk::Button {
        self.widget.remove(&self.header);
        self.widget.remove(&self.footer);
        self.footer.remove(&self.actions);
        self.widget.add_css_class("attachment-openable");
        let space = super::horizontal("attachment-action-space", 0);
        let sizes = gtk::SizeGroup::new(gtk::SizeGroupMode::Both);
        sizes.add_widget(&space);
        sizes.add_widget(&self.actions);
        self.footer.append(&space);
        self.footer.set_vexpand(true);
        self.footer.set_valign(gtk::Align::End);
        let content = super::column("attachment-open-content");
        content.append(&self.header);
        content.append(&self.footer);
        let button = gtk::Button::builder()
            .child(&content)
            .tooltip_text(format!("Open {name}"))
            .hexpand(true)
            .vexpand(true)
            .css_classes(["flat", "attachment-open"])
            .build();
        let overlay = gtk::Overlay::new();
        overlay.set_vexpand(true);
        overlay.set_child(Some(&button));
        let actions = super::horizontal("attachment-overlay-actions", 0);
        actions.set_halign(gtk::Align::End);
        actions.set_valign(gtk::Align::End);
        actions.append(&self.actions);
        overlay.add_overlay(&actions);
        self.widget.append(&overlay);
        button
    }
}

pub struct Section {
    pub widget: gtk::Box,
    #[cfg(test)]
    pub expander: gtk::Expander,
    pub grid: gtk::FlowBox,
    title: gtk::Label,
    summary: gtk::Label,
}

impl Section {
    pub fn new() -> Self {
        let widget = super::column("attachment-section");
        let title = super::label("Attachments", "heading");
        let summary = super::label("", "dim-label");
        summary.set_hexpand(false);
        let header = super::horizontal("attachment-heading", theme::SMALL_SPACING);
        header.append(&gtk::Image::from_icon_name("mail-attachment-symbolic"));
        header.append(&title);
        header.append(&summary);
        let grid = grid();
        let expander = gtk::Expander::builder()
            .label_widget(&header)
            .child(&grid)
            .expanded(false)
            .hexpand(true)
            .build();
        widget.append(&expander);
        Self {
            widget,
            #[cfg(test)]
            expander,
            grid,
            title,
            summary,
        }
    }

    pub fn update(&self, count: usize, total: Option<u64>) {
        self.widget.set_visible(count > 0);
        self.title.set_text(&format!("Attachments ({count})"));
        self.summary.set_text(
            &total
                .map(|size| gtk::glib::format_size(size).to_string())
                .unwrap_or_default(),
        );
    }

    pub fn append(&self, card: &FileCard) {
        self.grid.append(&card.widget);
    }
}

pub(super) fn grid() -> gtk::FlowBox {
    gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .activate_on_single_click(false)
        .homogeneous(true)
        .min_children_per_line(1)
        .max_children_per_line(theme::ATTACHMENT_MAX_COLUMNS)
        .column_spacing(theme::SMALL_SPACING as u32)
        .row_spacing(theme::SMALL_SPACING as u32)
        .hexpand(true)
        .css_classes(["attachment-grid"])
        .build()
}

pub fn view(message: &Message, account: Option<&Account>, folder: &str) -> gtk::Box {
    let section = Section::new();
    let visible: Vec<_> = message
        .attachments
        .iter()
        .enumerate()
        .filter(|(index, _)| !crate::models::attachment::rendered_calendar(message, *index))
        .collect();
    let total: Option<u64> = visible
        .iter()
        .map(|(index, _)| {
            message
                .attachment_details
                .get(*index)
                .map(|details| details.size)
        })
        .sum();
    section.update(visible.len(), total);
    for (index, name) in visible {
        let card = FileCard::new(
            name,
            message
                .attachment_details
                .get(index)
                .map(|details| details.size),
        );
        super::attachment_actions::attach(&card, account, folder, message, index);
        section.append(&card);
    }
    section.widget
}

pub fn cancelled(error: &gtk::glib::Error) -> bool {
    error.matches(gtk::DialogError::Dismissed) || error.matches(gtk::DialogError::Cancelled)
}
