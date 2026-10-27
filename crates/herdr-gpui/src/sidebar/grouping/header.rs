//! The heading a group paints over its hosts. Clicking it folds the group.

use super::Section;
use crate::{
    HerdrWindow,
    sidebar::{HOST_ARROW_WIDTH, label_text, layout::SidebarLook, line_height},
};
use gpui::{prelude::*, *};

impl HerdrWindow {
    /// A group's heading row: an arrow that shows whether it is folded, its
    /// title, and how many sessions it holds.
    pub(in crate::sidebar) fn group_header(
        &self,
        section: &Section,
        look: SidebarLook,
        width: f32,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let font = &self.config.sidebar;
        let theme = &self.theme;
        let layout = look.density;
        let slug = section.key.slug();
        let key = section.key.clone();
        let gap = layout.host_gap();
        let count = format!("{}", section.hosts.len());
        let count_width = count.chars().count() as f32 * crate::sidebar::metrics::glyph_width(font);
        let label_width =
            (look.content_width(width) - HOST_ARROW_WIDTH - 2. * gap - count_width).max(0.);
        div()
            .id(SharedString::from(format!("group-{slug}")))
            .debug_selector(|| format!("group-{slug}"))
            .h(px(line_height(font) + 2. * layout.header_padding()))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(gap))
            .px(px(look.content_x()))
            .text_size(px(font.size))
            .text_color(rgb(theme.muted))
            .cursor_pointer()
            .child(
                div()
                    .w(px(HOST_ARROW_WIDTH))
                    .flex_none()
                    .child(label_text(if section.folded {
                        "\u{25b8}"
                    } else {
                        "\u{25be}"
                    })),
            )
            .child(
                div()
                    .debug_selector(|| format!("group-label-{slug}"))
                    .w(px(label_width))
                    .flex_none()
                    .truncate()
                    .child(label_text(&section.title)),
            )
            .child(
                div()
                    .debug_selector(|| format!("group-count-{slug}"))
                    .flex_none()
                    .child(count),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.toggle_group(key.clone(), cx);
            }))
    }
}
