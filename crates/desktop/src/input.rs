//! Tracks search and tool input separately so editing tool text never changes modes.

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum InputMode {
    #[default]
    Search,
    Calculator,
    Terminal,
}

#[derive(Default)]
pub struct InputState {
    pub mode: InputMode,
    pub text: String,
    previous_search: Option<String>,
}

impl InputState {
    /// Returns true when a search prefix was consumed and the edit control needs updating.
    pub fn update(&mut self, text: String) -> bool {
        if self.mode == InputMode::Search {
            let trimmed = text.trim_start();
            let mode = match trimmed.chars().next() {
                Some('=') => Some(InputMode::Calculator),
                Some('>') => Some(InputMode::Terminal),
                _ => None,
            };
            if let Some(mode) = mode {
                self.previous_search = Some(std::mem::take(&mut self.text));
                self.mode = mode;
                self.text = trimmed[1..].to_string();
                return true;
            }
        }
        self.text = text;
        false
    }

    /// Restores the search text that was present before entering a tool.
    pub fn leave_mode(&mut self) -> bool {
        if self.mode == InputMode::Search {
            return false;
        }
        self.mode = InputMode::Search;
        self.text = self.previous_search.take().unwrap_or_default();
        true
    }

    /// Starts an empty search when navigating to another launcher page.
    pub fn reset(&mut self) {
        self.mode = InputMode::Search;
        self.text.clear();
        self.previous_search = None;
    }
}

#[cfg(test)]
mod tests {
    use super::{InputMode, InputState};

    #[test]
    fn consumes_a_prefix_and_preserves_the_remaining_text() {
        for (input, mode, expected) in [
            ("=", InputMode::Calculator, ""),
            ("   >", InputMode::Terminal, ""),
            ("\u{3000}= (1 + 2)  ", InputMode::Calculator, " (1 + 2)  "),
            (
                "> Write-Output 'Hello' ",
                InputMode::Terminal,
                " Write-Output 'Hello' ",
            ),
        ] {
            let mut state = InputState::default();
            assert!(state.update(input.to_string()));
            assert_eq!(state.mode, mode);
            assert_eq!(state.text, expected);
        }
    }

    #[test]
    fn tool_body_stays_in_its_mode_even_when_empty_or_starting_with_a_prefix() {
        for (trigger, mode) in [("=", InputMode::Calculator), (">", InputMode::Terminal)] {
            let mut state = InputState::default();
            assert!(state.update(trigger.to_string()));
            for body in ["", ">", "= 1", "  > Write-Output 'MixedCase'  "] {
                assert!(!state.update(body.to_string()));
                assert_eq!(state.mode, mode);
                assert_eq!(state.text, body);
            }
        }
    }

    #[test]
    fn leaving_a_tool_restores_the_previous_search() {
        let mut state = InputState::default();
        assert!(!state.update("设备管理器  ".to_string()));
        assert!(!state.leave_mode());
        assert!(state.update("=1+2".to_string()));
        assert!(!state.update("".to_string()));
        assert!(state.leave_mode());
        assert_eq!(state.mode, InputMode::Search);
        assert_eq!(state.text, "设备管理器  ");
        assert!(!state.leave_mode());
        assert_eq!(state.text, "设备管理器  ");
    }

    #[test]
    fn repeated_transitions_and_reset_do_not_restore_stale_search_text() {
        let mut state = InputState::default();
        state.update("first".to_string());
        assert!(state.update(">Get-Date".to_string()));
        assert!(state.leave_mode());
        assert_eq!(state.text, "first");
        state.update("second".to_string());
        assert!(state.update("=2+2".to_string()));
        assert!(state.leave_mode());
        assert_eq!(state.text, "second");

        assert!(state.update(">Get-Date".to_string()));
        state.reset();
        assert_eq!(state.mode, InputMode::Search);
        assert_eq!(state.text, "");
        assert!(!state.leave_mode());
        assert!(state.update("=".to_string()));
        assert!(state.leave_mode());
        assert_eq!(state.text, "");

        state.update("ordinary > search".to_string());
        assert_eq!(state.mode, InputMode::Search);
        state.reset();
        assert_eq!(state.mode, InputMode::Search);
        assert_eq!(state.text, "");
    }
}
