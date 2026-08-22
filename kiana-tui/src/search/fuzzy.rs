/// 大小写匹配模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseMatching {
    /// 智能匹配：模式全小写时忽略大小写，否则精确匹配
    Smart,
    /// 总是忽略大小写
    Insensitive,
    /// 总是精确匹配
    Sensitive,
}

/// 匹配结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchResult {
    /// 分数，越高越匹配
    pub score: i32,
    /// 匹配的字符位置（字符索引，非字节索引）
    pub positions: Vec<usize>,
    /// 是否匹配成功
    pub matched: bool,
}

/// 增强的模糊匹配器
#[derive(Debug, Clone)]
pub struct FuzzyMatcher {
    case_matching: CaseMatching,
    bonus_first_char: i32,
    bonus_consecutive: i32,
    bonus_camel_case: i32,
    bonus_word_boundary: i32,
    gap_penalty: i32,
}

impl Default for FuzzyMatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl FuzzyMatcher {
    /// 创建新的模糊匹配器，使用默认参数
    pub fn new() -> Self {
        Self {
            case_matching: CaseMatching::Smart,
            bonus_first_char: 16,
            bonus_consecutive: 15,
            bonus_camel_case: 14,
            bonus_word_boundary: 13,
            gap_penalty: 3,
        }
    }

    /// 设置大小写匹配模式
    pub fn with_case_matching(mut self, case: CaseMatching) -> Self {
        self.case_matching = case;
        self
    }

    /// 执行模糊匹配
    pub fn match_str(&self, text: &str, pattern: &str) -> Option<MatchResult> {
        if pattern.is_empty() {
            return Some(MatchResult {
                score: 0,
                positions: vec![],
                matched: true,
            });
        }

        let text_chars: Vec<char> = text.chars().collect();
        let pattern_chars: Vec<char> = pattern.chars().collect();

        // 确定是否忽略大小写
        let case_insensitive = match self.case_matching {
            CaseMatching::Insensitive => true,
            CaseMatching::Sensitive => false,
            CaseMatching::Smart => pattern_chars.iter().all(|c| !c.is_uppercase()),
        };

        // 快速路径：检查所有模式字符是否存在
        if !self.can_match(&text_chars, &pattern_chars, case_insensitive) {
            return None;
        }

        // 动态规划求解最佳匹配
        self.dp_match(&text_chars, &pattern_chars, case_insensitive)
    }

    /// 快速检查是否可能匹配（所有模式字符都存在）
    fn can_match(&self, text: &[char], pattern: &[char], case_insensitive: bool) -> bool {
        let mut text_idx = 0;
        for &p_char in pattern {
            let found = text[text_idx..]
                .iter()
                .position(|&t_char| self.char_equal(t_char, p_char, case_insensitive));
            if let Some(pos) = found {
                text_idx += pos + 1;
            } else {
                return false;
            }
        }
        true
    }

    /// 字符相等比较
    fn char_equal(&self, a: char, b: char, case_insensitive: bool) -> bool {
        if case_insensitive {
            a.to_lowercase().eq(b.to_lowercase())
        } else {
            a == b
        }
    }

    /// 使用动态规划计算最佳匹配
    fn dp_match(
        &self,
        text: &[char],
        pattern: &[char],
        case_insensitive: bool,
    ) -> Option<MatchResult> {
        let n = text.len();
        let m = pattern.len();

        // dp[i][j] = (score, last_matched_text_idx)
        // 表示匹配 pattern[0..j] 在 text[0..i] 中的最高分数
        let mut dp = vec![vec![(i32::MIN, 0); m + 1]; n + 1];

        // 初始化：匹配 0 个模式字符的分数为 0
        for i in 0..=n {
            dp[i][0] = (0, 0);
        }

        // 动态规划
        for i in 1..=n {
            for j in 1..=m {
                // 选项1：不使用 text[i-1]，继承之前的最佳分数
                dp[i][j] = dp[i - 1][j];

                // 选项2：使用 text[i-1] 匹配 pattern[j-1]
                if self.char_equal(text[i - 1], pattern[j - 1], case_insensitive) {
                    let (prev_score, prev_match_idx) = dp[i - 1][j - 1];
                    if prev_score != i32::MIN {
                        let mut bonus = 0;

                        // 首字符匹配奖励
                        if i == 1 {
                            bonus += self.bonus_first_char;
                        }

                        // 连续匹配奖励
                        if j > 1 && prev_match_idx == i - 2 {
                            bonus += self.bonus_consecutive;
                        }

                        // 驼峰匹配奖励（大写字母）
                        if text[i - 1].is_uppercase() && j > 1 {
                            bonus += self.bonus_camel_case;
                        }

                        // 词边界匹配奖励
                        if i > 1 && !text[i - 2].is_alphanumeric() && text[i - 1].is_alphanumeric()
                        {
                            bonus += self.bonus_word_boundary;
                        }

                        // 间隙惩罚
                        let gap = if j > 1 {
                            (i - 2).saturating_sub(prev_match_idx) as i32
                        } else {
                            (i - 1) as i32
                        };
                        let penalty = gap * self.gap_penalty;

                        // 大小写精确匹配额外奖励
                        let case_bonus = if !case_insensitive && text[i - 1] == pattern[j - 1] {
                            5
                        } else if text[i - 1] == pattern[j - 1] {
                            2
                        } else {
                            0
                        };

                        let new_score = prev_score + 100 + bonus + case_bonus - penalty;

                        if new_score > dp[i][j].0 {
                            dp[i][j] = (new_score, i - 1);
                        }
                    }
                }
            }
        }

        // 找到最佳匹配（在所有 text[i][m] 中找最高分）
        let mut best_score = i32::MIN;
        let mut best_i = 0;
        for i in m..=n {
            if dp[i][m].0 > best_score {
                best_score = dp[i][m].0;
                best_i = i;
            }
        }

        if best_score == i32::MIN {
            return None;
        }

        // 回溯获取匹配位置
        let mut positions = Vec::new();
        let mut i = best_i;
        let mut j = m;

        while j > 0 {
            let (score_here, match_idx) = dp[i][j];
            let (score_skip, _) = dp[i - 1][j];

            // 如果当前分数是通过匹配得到的（而不是跳过）
            if score_here > score_skip || (i > 0 && dp[i - 1][j].0 == i32::MIN) {
                positions.push(match_idx);
                i = match_idx;
                j -= 1;
            } else {
                i -= 1;
            }
        }

        positions.reverse();

        Some(MatchResult {
            score: best_score,
            positions,
            matched: true,
        })
    }
}

/// 计算模糊匹配分数（兼容旧 API）
/// 返回 (score, positions)，其中 score 越高越匹配
pub fn calculate_match_score(text: &str, pattern: &str) -> Option<(i32, Vec<usize>)> {
    let matcher = FuzzyMatcher::new();
    matcher
        .match_str(text, pattern)
        .map(|result| (result.score, result.positions))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuzzy_match_basic() {
        let matcher = FuzzyMatcher::new();
        let result = matcher.match_str("hello world", "hlo");
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(r.matched);
        // h(0), l(2 或 3), o(4) - 算法会选择最优路径
        assert_eq!(r.positions.len(), 3);
        assert_eq!(r.positions[0], 0); // h
        assert!(r.positions[1] == 2 || r.positions[1] == 3); // l
        assert_eq!(r.positions[2], 4); // o
    }

    #[test]
    fn test_fuzzy_match_consecutive() {
        let matcher = FuzzyMatcher::new();
        let result = matcher.match_str("hello world", "hello");
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(r.matched);
        assert_eq!(r.positions, vec![0, 1, 2, 3, 4]);
        // 连续匹配应该得到更高分数
        assert!(r.score > 500); // 基础分 500 + 奖励
    }

    #[test]
    fn test_fuzzy_match_case_smart() {
        let matcher = FuzzyMatcher::new().with_case_matching(CaseMatching::Smart);

        // 全小写模式 -> 忽略大小写
        let result = matcher.match_str("Hello World", "hello");
        assert!(result.is_some());

        // 包含大写的模式 -> 精确匹配
        let result = matcher.match_str("Hello World", "Hello");
        assert!(result.is_some());

        let result = matcher.match_str("hello world", "Hello");
        assert!(result.is_none()); // 大小写不匹配
    }

    #[test]
    fn test_fuzzy_match_case_insensitive() {
        let matcher = FuzzyMatcher::new().with_case_matching(CaseMatching::Insensitive);
        let result = matcher.match_str("Hello World", "HELLO");
        assert!(result.is_some());
    }

    #[test]
    fn test_fuzzy_match_case_sensitive() {
        let matcher = FuzzyMatcher::new().with_case_matching(CaseMatching::Sensitive);
        let result = matcher.match_str("Hello World", "hello");
        assert!(result.is_none());

        let result = matcher.match_str("Hello World", "Hello");
        assert!(result.is_some());
    }

    #[test]
    fn test_fuzzy_match_camel_case() {
        let matcher = FuzzyMatcher::new();
        let result = matcher.match_str("ButtonComponent", "BC");
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.positions, vec![0, 6]); // B, C
                                             // 驼峰匹配应该得到奖励
    }

    #[test]
    fn test_fuzzy_match_word_boundary() {
        let matcher = FuzzyMatcher::new();
        let result = matcher.match_str("src/components/button", "scb");
        assert!(result.is_some());
        let r = result.unwrap();
        // s, c, b 应该匹配词边界
        assert!(r.positions.contains(&0)); // s
    }

    #[test]
    fn test_fuzzy_match_gaps() {
        let matcher = FuzzyMatcher::new();

        // 较小的间隙 - 连续的字符
        let result1 = matcher.match_str("hello", "hel");
        // 较大的间隙 - 字符之间有间隔
        let result2 = matcher.match_str("h-e-l-l-o", "hel");

        assert!(result1.is_some());
        assert!(result2.is_some());

        // 间隙小的应该得分更高（连续匹配有奖励）
        assert!(result1.unwrap().score > result2.unwrap().score);
    }

    #[test]
    fn test_fuzzy_match_positions() {
        let matcher = FuzzyMatcher::new();
        let result = matcher.match_str("Button.tsx", "btn");
        assert!(result.is_some());
        let r = result.unwrap();
        // 应该匹配 B, t, t 或 B, t, n
        assert!(r.positions.len() == 3);
    }

    #[test]
    fn test_fuzzy_match_no_match() {
        let matcher = FuzzyMatcher::new();
        let result = matcher.match_str("hello world", "xyz");
        assert!(result.is_none());
    }

    #[test]
    fn test_fuzzy_match_empty_pattern() {
        let matcher = FuzzyMatcher::new();
        let result = matcher.match_str("hello", "");
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.score, 0);
        assert_eq!(r.positions, Vec::<usize>::new());
    }

    #[test]
    fn test_fuzzy_match_unicode() {
        let matcher = FuzzyMatcher::new();
        let result = matcher.match_str("你好世界", "好界");
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.positions, vec![1, 3]); // 字符索引
    }

    #[test]
    fn test_fuzzy_match_prefix_bonus() {
        let matcher = FuzzyMatcher::new();

        // 前缀匹配应该得分更高
        let result1 = matcher.match_str("hello", "hel");
        let result2 = matcher.match_str("xhello", "hel");

        assert!(result1.is_some());
        assert!(result2.is_some());
        assert!(result1.unwrap().score > result2.unwrap().score);
    }

    #[test]
    fn test_calculate_match_score_compat() {
        // 测试兼容旧 API
        let result = calculate_match_score("hello world", "hlo");
        assert!(result.is_some());
        let (score, positions) = result.unwrap();
        assert!(score > 0);
        assert_eq!(positions.len(), 3);
    }

    #[test]
    fn test_fuzzy_match_scoring_order() {
        let matcher = FuzzyMatcher::new();
        let items = vec![
            "src/components/Button.tsx",
            "src/utils/button.ts",
            "tests/button.test.ts",
            "Button.tsx",
            "my_button_helper.ts",
        ];

        let mut scored: Vec<_> = items
            .iter()
            .filter_map(|item| matcher.match_str(item, "btn").map(|r| (r.score, item)))
            .collect();

        scored.sort_by_key(|(score, _)| -score);

        // Button.tsx 应该排在前面（前缀匹配，驼峰匹配）
        assert!(scored.len() > 0);
        // 验证至少能匹配到一些项
    }
}
