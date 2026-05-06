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
}
