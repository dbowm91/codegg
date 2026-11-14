#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScrollPolicy {
    #[default]
    Clamp,
    Wrap,
}

#[derive(Debug, Clone)]
pub struct ListCursor {
    index: usize,
    offset: CenteredScroll,
    policy: ScrollPolicy,
}

impl Default for ListCursor {
    fn default() -> Self {
        Self::new()
    }
}

impl ListCursor {
    pub fn new() -> Self {
        Self::with_policy(ScrollPolicy::Clamp)
    }

    pub fn with_policy(policy: ScrollPolicy) -> Self {
        Self {
            index: 0,
            offset: CenteredScroll::new(),
            policy,
        }
    }

    pub fn policy(&self) -> ScrollPolicy {
        self.policy
    }

    pub fn index(&self, len: usize) -> usize {
        self.index.min(len.saturating_sub(1))
    }

    pub fn offset(&self) -> usize {
        self.offset.get()
    }

    pub fn set_index(&mut self, index: usize) {
        self.index = index;
    }

    pub fn reset(&mut self) {
        self.index = 0;
        self.offset.reset();
    }

    pub fn place(&mut self, index: usize, len: usize, visible: usize) -> usize {
        self.index = index.min(len.saturating_sub(1));
        self.follow(len, visible);
        self.index
    }

    pub fn up(&mut self, len: usize, visible: usize) -> usize {
        self.step(false, len, visible)
    }

    pub fn down(&mut self, len: usize, visible: usize) -> usize {
        self.step(true, len, visible)
    }

    pub fn page_up(&mut self, len: usize, visible: usize) -> usize {
        self.page(false, len, visible)
    }

    pub fn page_down(&mut self, len: usize, visible: usize) -> usize {
        self.page(true, len, visible)
    }

    pub fn home(&mut self, len: usize, visible: usize) -> usize {
        self.place(0, len, visible)
    }

    pub fn end(&mut self, len: usize, visible: usize) -> usize {
        self.place(len.saturating_sub(1), len, visible)
    }

    fn step(&mut self, forward: bool, len: usize, visible: usize) -> usize {
        self.step_index(forward, len);
        self.follow(len, visible);
        self.index
    }

    fn step_index(&mut self, forward: bool, len: usize) {
        let current = self.index(len);
        self.index = if len == 0 {
            0
        } else {
            match (forward, self.policy) {
                (true, ScrollPolicy::Clamp) => current.saturating_add(1).min(len - 1),
                (false, ScrollPolicy::Clamp) => current.saturating_sub(1),
                (true, ScrollPolicy::Wrap) => (current + 1) % len,
                (false, ScrollPolicy::Wrap) => (current + len - 1) % len,
            }
        };
    }

    fn page(&mut self, forward: bool, len: usize, visible: usize) -> usize {
        for _ in 0..visible.max(1) {
            self.step_index(forward, len);
        }
        self.follow(len, visible);
        self.index
    }

    fn follow(&mut self, len: usize, visible: usize) {
        let offset = self.offset.get();
        if self.index < offset || self.index >= offset.saturating_add(visible) {
            self.offset.clamp(self.index, len, visible);
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CenteredScroll {
    scroll: usize,
}

impl CenteredScroll {
    pub fn new() -> Self {
        Self { scroll: 0 }
    }

    pub fn reset(&mut self) {
        self.scroll = 0;
    }

    pub fn get(&self) -> usize {
        self.scroll
    }

    pub fn set(&mut self, scroll: usize) {
        self.scroll = scroll;
    }

    pub fn clamp(&mut self, cursor: usize, total: usize, visible: usize) {
        if total == 0 || visible == 0 {
            self.scroll = 0;
            return;
        }

        let max_scroll = total.saturating_sub(visible);
        let middle = visible / 2;

        let new_scroll = if cursor >= max_scroll {
            max_scroll
        } else if cursor < middle {
            0
        } else {
            cursor.saturating_sub(middle)
        };

        self.scroll = new_scroll.min(max_scroll);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scroll_at_top() {
        let mut scroll = CenteredScroll::new();
        scroll.clamp(0, 20, 8);
        assert_eq!(scroll.get(), 0);
    }

    #[test]
    fn test_scroll_centered() {
        let mut scroll = CenteredScroll::new();
        scroll.clamp(10, 20, 8);
        assert_eq!(scroll.get(), 6);
    }

    #[test]
    fn test_scroll_at_max() {
        let mut scroll = CenteredScroll::new();
        scroll.clamp(19, 20, 8);
        assert_eq!(scroll.get(), 12);
    }

    #[test]
    fn test_short_list() {
        let mut scroll = CenteredScroll::new();
        scroll.clamp(0, 3, 8);
        assert_eq!(scroll.get(), 0);
    }

    #[test]
    fn test_selection_at_exact_middle_boundary() {
        let mut scroll = CenteredScroll::new();
        scroll.clamp(4, 10, 8);
        assert_eq!(scroll.get(), 2);
    }

    #[test]
    fn test_selection_at_last_visible_item() {
        let mut scroll = CenteredScroll::new();
        scroll.clamp(7, 20, 8);
        assert_eq!(scroll.get(), 3);
    }

    #[test]
    fn test_single_item_list() {
        let mut scroll = CenteredScroll::new();
        scroll.clamp(0, 1, 8);
        assert_eq!(scroll.get(), 0);
    }

    #[test]
    fn test_cursor_at_max_scroll_boundary() {
        let mut scroll = CenteredScroll::new();
        scroll.clamp(12, 20, 8);
        assert_eq!(scroll.get(), 12);
    }

    #[test]
    fn test_visible_equals_total() {
        let mut scroll = CenteredScroll::new();
        scroll.clamp(0, 5, 5);
        assert_eq!(scroll.get(), 0);
    }

    #[test]
    fn test_cursor_near_end() {
        let mut scroll = CenteredScroll::new();
        scroll.clamp(18, 20, 8);
        assert_eq!(scroll.get(), 12);
    }

    #[test]
    fn cursor_defaults_to_the_clamp_policy() {
        assert_eq!(ListCursor::new().policy(), ScrollPolicy::Clamp);
        assert_eq!(ListCursor::default().policy(), ScrollPolicy::Clamp);
        assert_eq!(
            ListCursor::with_policy(ScrollPolicy::Wrap).policy(),
            ScrollPolicy::Wrap
        );
    }

    #[test]
    fn an_empty_list_keeps_the_cursor_at_zero() {
        let mut cursor = ListCursor::new();
        assert_eq!(cursor.down(0, 8), 0);
        assert_eq!(cursor.up(0, 8), 0);
        assert_eq!(cursor.page_down(0, 8), 0);
        assert_eq!(cursor.end(0, 8), 0);
        assert_eq!(cursor.offset(), 0);
    }

    #[test]
    fn a_single_item_list_cannot_move() {
        let mut cursor = ListCursor::new();
        assert_eq!(cursor.down(1, 8), 0);
        assert_eq!(cursor.up(1, 8), 0);
        assert_eq!(cursor.page_down(1, 8), 0);
    }

    #[test]
    fn clamp_stops_at_the_first_item_but_wrap_returns_to_the_last() {
        let mut clamp = ListCursor::with_policy(ScrollPolicy::Clamp);
        let mut wrap = ListCursor::with_policy(ScrollPolicy::Wrap);
        assert_eq!(clamp.up(5, 8), 0);
        assert_eq!(
            wrap.up(5, 8),
            4,
            "ScrollPolicy::Wrap must leave the first item for the last"
        );
    }

    #[test]
    fn clamp_stops_at_the_last_item_but_wrap_returns_to_the_first() {
        let mut clamp = ListCursor::with_policy(ScrollPolicy::Clamp);
        let mut wrap = ListCursor::with_policy(ScrollPolicy::Wrap);
        assert_eq!(clamp.end(5, 8), 4);
        assert_eq!(
            clamp.down(5, 8),
            4,
            "ScrollPolicy::Clamp must stay on the last item"
        );
        assert_eq!(
            wrap.end(5, 8),
            4,
            "sitting on the last item must not itself wrap"
        );
        assert_eq!(
            wrap.down(5, 8),
            0,
            "ScrollPolicy::Wrap must leave the last item for the first"
        );
    }

    #[test]
    fn home_and_end_reach_both_ends_in_one_step() {
        let mut cursor = ListCursor::new();
        assert_eq!(cursor.end(20, 8), 19);
        assert_eq!(cursor.home(20, 8), 0);
    }

    #[test]
    fn a_page_steps_one_visible_window_and_clamps() {
        let mut cursor = ListCursor::new();
        assert_eq!(cursor.page_down(20, 8), 8);
        assert_eq!(cursor.page_down(20, 8), 16);
        assert_eq!(
            cursor.page_down(20, 8),
            19,
            "a page past the end must clamp, not wrap"
        );
        assert_eq!(cursor.page_up(20, 8), 11);
    }

    #[test]
    fn a_page_of_a_zero_height_window_still_moves_one_row() {
        let mut cursor = ListCursor::new();
        assert_eq!(cursor.page_down(20, 0), 1);
        assert_eq!(cursor.page_up(20, 0), 0);
    }

    #[test]
    fn a_window_taller_than_the_list_never_scrolls() {
        let mut cursor = ListCursor::new();
        cursor.end(3, 8);
        assert_eq!(cursor.index(3), 2);
        assert_eq!(cursor.offset(), 0);
    }

    #[test]
    fn a_zero_height_window_pins_the_offset_to_the_top() {
        let mut cursor = ListCursor::new();
        cursor.end(20, 0);
        assert_eq!(cursor.index(20), 19);
        assert_eq!(cursor.offset(), 0);
    }

    #[test]
    fn leaving_the_window_scrolls_and_staying_inside_it_does_not() {
        let mut cursor = ListCursor::new();
        for _ in 0..8 {
            cursor.down(20, 8);
        }
        assert_eq!(cursor.index(20), 8);
        assert_eq!(
            cursor.offset(),
            4,
            "the window follows only when the cursor leaves it"
        );

        cursor.up(20, 8);
        assert_eq!(
            cursor.offset(),
            4,
            "a step inside the window must not move the window"
        );
    }

    #[test]
    fn a_shrinking_list_clamps_a_stale_cursor_on_read() {
        let mut cursor = ListCursor::new();
        cursor.set_index(9);
        assert_eq!(
            cursor.index(3),
            2,
            "a stale index must resolve to the last real row, not point past the end"
        );
        assert_eq!(cursor.index(0), 0);
    }

    #[test]
    fn a_shrinking_list_re_seats_the_cursor_on_the_next_step() {
        let mut cursor = ListCursor::new();
        cursor.set_index(9);
        assert_eq!(
            cursor.down(3, 8),
            2,
            "the first step after a shrink must land on the last real row"
        );
    }

    #[test]
    fn place_moves_the_window_only_when_the_target_is_outside_it() {
        let mut cursor = ListCursor::new();
        cursor.place(2, 20, 8);
        assert_eq!(cursor.offset(), 0);
        cursor.place(10, 20, 8);
        assert_eq!(cursor.offset(), 6);
    }

    #[test]
    fn reset_returns_both_the_index_and_the_window_to_the_top() {
        let mut cursor = ListCursor::new();
        cursor.end(20, 8);
        cursor.reset();
        assert_eq!(cursor.index(20), 0);
        assert_eq!(cursor.offset(), 0);
    }
}
