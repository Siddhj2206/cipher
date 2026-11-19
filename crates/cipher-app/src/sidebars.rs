use crate::theme::{GUTTER_W, SCRUBBER_W};

const MIN_TEXT: f32 = 520.0;
pub const PAGE_CHROME: f32 = GUTTER_W + 48.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sidebar {
    Contents,
    Context,
}

impl Sidebar {
    pub const fn default_width(self) -> f32 {
        match self {
            Self::Contents => 272.0,
            Self::Context => 320.0,
        }
    }

    pub const fn min_width(self) -> f32 {
        match self {
            Self::Contents => 224.0,
            Self::Context => 256.0,
        }
    }

    pub const MAX_WIDTH: f32 = 560.0;
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SidebarLayout {
    pub contents: Option<f32>,
    pub context: Option<f32>,
}

impl SidebarLayout {
    fn room(window: f32) -> f32 {
        window - MIN_TEXT - PAGE_CHROME
    }

    /// Open preferences survive auto-collapse. The last-opened pane gets first claim on
    /// the space, and may fit at less than its preferred width when alone.
    pub fn fit(
        window: f32,
        contents: Option<f32>,
        context: Option<f32>,
        context_first: bool,
    ) -> Self {
        let contents = contents.map(|w| w.clamp(Sidebar::Contents.min_width(), Sidebar::MAX_WIDTH));
        let context = context.map(|w| w.clamp(Sidebar::Context.min_width(), Sidebar::MAX_WIDTH));
        let room = Self::room(window);
        if matches!((contents, context), (Some(l), Some(r)) if l + r <= room) {
            return Self { contents, context };
        }
        let left = contents
            .filter(|_| room >= Sidebar::Contents.min_width())
            .map(|w| w.min(room));
        let right_room = room - SCRUBBER_W;
        let right = context
            .filter(|_| right_room >= Sidebar::Context.min_width())
            .map(|w| w.min(right_room));
        if right.is_some() && (context_first || left.is_none()) {
            Self {
                contents: None,
                context: right,
            }
        } else {
            Self {
                contents: left,
                context: None,
            }
        }
    }

    pub fn width(self, side: Sidebar) -> Option<f32> {
        match side {
            Sidebar::Contents => self.contents,
            Sidebar::Context => self.context,
        }
    }

    pub fn insets(self) -> (f32, f32) {
        (
            self.contents.unwrap_or(SCRUBBER_W),
            self.context.unwrap_or(0.0),
        )
    }

    /// A divider may fill spare room, but never at the expense of the other visible pane.
    pub fn max_width(self, side: Sidebar, window: f32) -> f32 {
        let (left, right) = self.insets();
        let opposite = match side {
            Sidebar::Contents => right,
            Sidebar::Context => left,
        };
        let room = Self::room(window);
        let max = (room - opposite).clamp(side.min_width(), Sidebar::MAX_WIDTH);
        if max > side.min_width() && max + opposite > room {
            max.next_down()
        } else {
            max
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_opened_wins_then_both_preferred_widths_return() {
        let fit = |w, context_first| SidebarLayout::fit(w, Some(370.0), Some(400.0), context_first);
        let left = fit(1000.0, false);
        assert_eq!(left.contents, Some(370.0));
        assert_eq!(left.context, None);
        let right = fit(1000.0, true);
        assert_eq!(right.contents, None);
        assert_eq!(right.context, Some(332.0));
        let wide = fit(1480.0, true);
        assert_eq!(wide.contents, Some(370.0));
        assert_eq!(wide.context, Some(400.0));
    }

    #[test]
    fn resize_leaves_room_for_the_page_and_other_sidebar() {
        let layout = SidebarLayout::fit(1340.0, Some(272.0), Some(320.0), false);
        assert_eq!(layout.max_width(Sidebar::Contents, 1340.0), 396.0);
        assert_eq!(layout.max_width(Sidebar::Context, 1340.0), 444.0);
        assert_eq!(
            layout.max_width(Sidebar::Context, 4000.0),
            Sidebar::MAX_WIDTH
        );
        let solo = SidebarLayout::fit(930.0, None, Some(320.0), true);
        assert_eq!(solo.context, Some(262.0));
        assert_eq!(solo.max_width(Sidebar::Context, 930.0), 262.0);
    }

    #[test]
    fn fractional_width_at_max_keeps_both_sidebars() {
        let window = 1428.87;
        let contents = Some(283.737_5);
        let layout = SidebarLayout::fit(window, contents, Some(320.0), false);
        let max = layout.max_width(Sidebar::Context, window);
        assert_eq!(
            SidebarLayout::fit(window, contents, Some(max), false),
            SidebarLayout {
                contents,
                context: Some(max)
            },
        );
    }

    #[test]
    fn keeps_contents_when_priority_context_cannot_fit() {
        let layout = SidebarLayout::fit(880.0, Some(272.0), Some(320.0), true);
        assert_eq!(
            layout,
            SidebarLayout {
                contents: Some(256.0),
                context: None
            }
        );
    }

    #[test]
    fn bounds_stale_prefs_and_collapses_when_even_one_sidebar_would_not_fit() {
        let wide = SidebarLayout::fit(2000.0, Some(-20.0), Some(800.0), false);
        assert_eq!(wide.contents, Some(Sidebar::Contents.min_width()));
        assert_eq!(wide.context, Some(Sidebar::MAX_WIDTH));
        for zoom in crate::prefs::Prefs::ZOOMS {
            let window = crate::prefs::WindowState::MIN.0 / zoom;
            let narrow = SidebarLayout::fit(window, Some(272.0), Some(320.0), false);
            let (left, right) = narrow.insets();
            assert!(right == 0.0 || window - left - right - PAGE_CHROME >= MIN_TEXT);
            assert!(narrow.contents.is_none() || window - left - right - PAGE_CHROME >= MIN_TEXT);
        }
        assert_eq!(
            SidebarLayout::fit(760.0 / 1.6, Some(272.0), Some(320.0), true),
            SidebarLayout::default(),
        );
    }
}
