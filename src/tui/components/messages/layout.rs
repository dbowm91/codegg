#[derive(Clone, Default)]
pub struct MessageLayoutCache {
    message_offsets: Vec<(usize, usize, usize)>,
    total_lines: usize,
}

impl MessageLayoutCache {
    pub fn new(offsets: Vec<(usize, usize, usize)>, total: usize) -> Self {
        Self {
            message_offsets: offsets,
            total_lines: total,
        }
    }

    pub fn find_message_at_line(&self, target: usize) -> Option<(usize, usize)> {
        if self.message_offsets.is_empty() || target >= self.total_lines {
            return None;
        }
        let mut left = 0;
        let mut right = self.message_offsets.len();
        while left < right {
            let mid = left + (right - left) / 2;
            let (_, start, count) = self.message_offsets[mid];
            if start + count <= target {
                left = mid + 1;
            } else {
                right = mid;
            }
        }
        if left >= self.message_offsets.len() {
            return None;
        }
        let (msg_idx, start, count) = self.message_offsets[left];
        Some((msg_idx, target - start))
    }

    pub fn find_visible_range(&self, scroll: usize, visible_height: usize) -> (usize, usize) {
        let start_idx = self
            .find_message_at_line(scroll)
            .map(|(idx, _)| idx.saturating_sub(2))
            .unwrap_or(0);
        let end_line = scroll.saturating_add(visible_height);
        let end_idx = self
            .find_message_at_line(end_line)
            .map(|(idx, _)| idx + 2)
            .unwrap_or(self.message_offsets.len());
        (
            start_idx,
            end_idx.min(self.message_offsets.len()),
        )
    }

    pub fn total_lines(&self) -> usize {
        self.total_lines
    }

    pub fn get_offset(&self, msg_idx: usize) -> Option<usize> {
        self.message_offsets
            .iter()
            .find(|(idx, _, _)| *idx == msg_idx)
            .map(|(_, start, _)| *start)
    }
}
