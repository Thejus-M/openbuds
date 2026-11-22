//! Battery text rules, ported from `Model.batteryText` and `Model.levelText`
//! in the old home.buds widget.

/// The bar text: the lowest non-zero earbud level, with `+` while charging.
/// `None` when no earbud reports a level.
pub fn bar_text(left: u8, right: u8, charging: bool) -> Option<String> {
    let lowest = [left, right].into_iter().filter(|&v| v > 0).min()?;
    Some(format!("{lowest}%{}", if charging { "+" } else { "" }))
}

/// One level for a detail line. A value of 0 means the bud is in the case
/// and reports nothing, so it shows a dash, not a false 0%.
pub fn level_text(value: u8) -> String {
    if value == 0 { "—".into() } else { format!("{value}%") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_text_uses_lowest_non_zero_level() {
        assert_eq!(bar_text(80, 55, false).as_deref(), Some("55%"));
    }

    #[test]
    fn bar_text_skips_zero_bud_in_case() {
        assert_eq!(bar_text(0, 42, false).as_deref(), Some("42%"));
    }

    #[test]
    fn bar_text_marks_charging() {
        assert_eq!(bar_text(30, 30, true).as_deref(), Some("30%+"));
    }

    #[test]
    fn bar_text_empty_when_no_level() {
        assert_eq!(bar_text(0, 0, false), None);
    }

    #[test]
    fn level_text_dash_for_zero() {
        assert_eq!(level_text(0), "—");
        assert_eq!(level_text(7), "7%");
    }
}
