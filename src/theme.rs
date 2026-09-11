pub const WINDOW_WIDTH: i32 = 1400;
pub const WINDOW_HEIGHT: i32 = 900;
pub const SIDEBAR_WIDTH: i32 = 300;
pub const LIST_WIDTH: i32 = 400;
pub const SPACING: i32 = 12;
pub const COMPOSE_HEIGHT: i32 = 200;
pub const BODY_HEIGHT: i32 = 50;
pub const SIDEBAR_HEADER_WIDTH: i32 = 350;
pub const ROW_GAP: i32 = 5;
pub const ROW_VERTICAL_GAP: i32 = 10;
pub const ICON_GAP: i32 = 24;
pub const INDENT: i32 = 45;
pub const SMALL_SPACING: i32 = 6;
pub const STATE_ICON: i32 = 48;
pub const AVATAR_SIZE: i32 = 32;
pub const SENDER_AVATAR_SIZE: i32 = 40;
pub const ACCOUNT_AVATAR_SIZE: i32 = 24;
pub const AVATAR_PIXELS: u32 = 128;
/// Brand icons are asked for larger than photos because some services hold a
/// high-resolution icon and hand back the biggest they have.
pub const ICON_PIXELS: u32 = 256;
pub const SYNC_SECONDS: u64 = 300;
pub const MESSAGE_LIMIT: usize = 50;
pub const CSS: &str = concat!(
    include_str!("components/ui/theme.css"),
    include_str!("components/button/style.css"),
    include_str!("components/container/style.css"),
    include_str!("components/content/style.css"),
    include_str!("components/sidebar/style.css"),
    include_str!("components/header/style.css"),
    include_str!("components/message_list/style.css"),
    include_str!("components/message_viewer/style.css"),
    include_str!("components/ui/style.css"),
    include_str!("components/html_viewer/style.css"),
    include_str!("theme.css"),
);
pub const HTML_CSS: &str = include_str!("components/html_viewer/body.css");
