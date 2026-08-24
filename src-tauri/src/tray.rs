//! Tray labels and hide/show policy for the collapsed ball.

pub fn ball_toggle_label(hidden: bool) -> &'static str {
    if hidden {
        "显示小球"
    } else {
        "隐藏小球"
    }
}

#[cfg(test)]
mod tests {
    use super::ball_toggle_label;

    #[test]
    fn toggle_label_follows_hidden_state() {
        assert_eq!(ball_toggle_label(false), "隐藏小球");
        assert_eq!(ball_toggle_label(true), "显示小球");
    }
}
