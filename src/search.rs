// SPDX-License-Identifier: GPL-3.0-only

use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::storage::HistoryItem;

pub const MAX_RENDERED_RESULTS: usize = 100;

pub fn rank(items: &[HistoryItem], query: &str) -> Vec<HistoryItem> {
    let query = query.trim();
    if query.is_empty() {
        return items.iter().take(MAX_RENDERED_RESULTS).cloned().collect();
    }

    let pattern = Pattern::new(
        query,
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Fuzzy,
    );
    let mut matcher = Matcher::new(Config::DEFAULT);
    let mut utf32_buffer = Vec::new();
    let mut scored = items
        .iter()
        .enumerate()
        .filter_map(|(position, item)| {
            let haystack = Utf32Str::new(&item.content, &mut utf32_buffer);
            pattern
                .score(haystack, &mut matcher)
                .map(|score| (score, position, item.clone()))
        })
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    scored
        .into_iter()
        .take(MAX_RENDERED_RESULTS)
        .map(|(_, _, item)| item)
        .collect()
}

pub fn preview(content: &str, max_chars: usize) -> String {
    let mut output = String::new();
    let mut previous_space = false;
    let mut truncated = false;

    for character in content.chars() {
        let normalized = match character {
            '\0' => '␀',
            value if value.is_control() || value.is_whitespace() => ' ',
            value => value,
        };
        if normalized == ' ' {
            if previous_space || output.is_empty() {
                continue;
            }
            previous_space = true;
        } else {
            previous_space = false;
        }
        if output.chars().count() == max_chars {
            truncated = true;
            break;
        }
        output.push(normalized);
    }

    let mut output = output.trim_end().to_owned();
    if truncated {
        output.push('…');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{preview, rank};
    use crate::storage::HistoryItem;

    fn item(id: i64, content: &str) -> HistoryItem {
        HistoryItem {
            id,
            content: content.to_owned(),
            created_at_ms: id,
        }
    }

    #[test]
    fn empty_query_preserves_recency_order() {
        let items = vec![item(3, "third"), item(2, "second")];
        assert_eq!(rank(&items, ""), items);
    }

    #[test]
    fn fuzzy_search_handles_unicode_and_case() {
        let items = vec![
            item(3, "Rust Wayland 剪贴板"),
            item(2, "中文开发笔记"),
            item(1, "unrelated"),
        ];
        assert_eq!(rank(&items, "rwl")[0].id, 3);
        assert_eq!(rank(&items, "开发")[0].id, 2);
        assert_eq!(rank(&items, "RUST")[0].id, 3);
        assert!(rank(&items, "完全不存在的内容").is_empty());
    }

    #[test]
    fn preview_flattens_without_changing_stored_content() {
        assert_eq!(preview("  one\n\ttwo\0three  ", 40), "one two␀three");
        assert_eq!(preview("abcdef", 3), "abc…");
    }
}
