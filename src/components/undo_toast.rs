pub fn new(title: &str, undo: impl Fn() + 'static) -> adw::Toast {
    let toast = adw::Toast::builder()
        .title(title)
        .button_label("Undo")
        .timeout(5)
        .build();
    toast.connect_button_clicked(move |_| undo());
    toast
}
