/// 计算模糊匹配分数
/// 返回 (score, positions)，其中 score 越小越匹配
pub fn calculate_match_score(text: &str, pattern: &str) -> Option<(usize, Vec<usize>)> {
    if pattern.is_empty() {
        return Some((0, vec![]));
    }

    let text_lower = text.to_lowercase();
    let pattern_lower = pattern.to_lowercase();

    // 简单子串匹配
    if let Some(start_pos) = text_lower.find(&pattern_lower) {
        let positions: Vec<usize> = (start_pos..start_pos + pattern.len()).collect();
        // 分数：起始位置（越早越好）
        let score = start_pos;
        Some((score, positions))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        let result = calculate_match_score("hello world", "hello");
        assert!(result.is_some());
        let (score, positions) = result.unwrap();
        assert_eq!(score, 0); // 起始位置为 0
        assert_eq!(positions, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn test_partial_match() {
        let result = calculate_match_score("hello world", "world");
        assert!(result.is_some());
        let (score, positions) = result.unwrap();
        assert_eq!(score, 6); // 起始位置为 6
        assert_eq!(positions, vec![6, 7, 8, 9, 10]);
    }

    #[test]
    fn test_no_match() {
        let result = calculate_match_score("hello world", "xyz");
        assert!(result.is_none());
    }

    #[test]
    fn test_case_insensitive() {
        let result = calculate_match_score("Hello World", "WORLD");
        assert!(result.is_some());
    }

    #[test]
    fn test_empty_pattern() {
        let result = calculate_match_score("hello", "");
        assert!(result.is_some());
        let (score, positions) = result.unwrap();
        assert_eq!(score, 0);
        assert_eq!(positions, vec![]);
    }
}
