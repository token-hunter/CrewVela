use std::num::NonZeroU16;
use std::time::{Duration, Instant};

use portable_pty::PtySize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Choice {
    ClaudeExit,
    ClaudeTrust,
    CodexRestricted,
    CodexSkipUntrustedHooks,
}

pub(super) fn choice(size: PtySize, transcript: &str) -> Option<Choice> {
    let cols = NonZeroU16::new(size.cols)?;
    let mut terminal = vt100::Parser::new(NonZeroU16::new(size.rows)?, cols, 0);
    terminal.process(transcript.as_bytes());
    let rows: Vec<_> = terminal.screen().rows(0, cols.get()).collect();
    let screen = rows.join(" ").split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_lowercase();
    let selected = |option: &str| {
        rows.iter().any(|row| {
            let row = row.trim_start();
            row.strip_prefix(['>', '›', '❯'])
                .is_some_and(|text| text.trim_start().to_ascii_lowercase().starts_with(option))
        })
    };
    if screen.contains("accessing workspace")
        && screen.contains("yes, i trust this folder")
        && screen.contains("enter to confirm")
    {
        if selected("yes, i trust this folder") || selected("1. yes, i trust this folder") {
            return Some(Choice::ClaudeTrust);
        }
        if selected("no, exit") && screen.contains("no, exit yes, i trust this folder") {
            return Some(Choice::ClaudeExit);
        }
    }
    if screen.contains("folder access")
        && screen.contains("untrusted folders stay disabled")
        && screen.contains("will not change saved trust")
        && screen.contains("enter continue")
        && selected("1. open restricted")
    {
        return Some(Choice::CodexRestricted);
    }
    // The startup picker explicitly advertises Esc as "skip". The same key
    // in a composer, approval or hook browser has a different meaning.
    if let Some((_, panel)) = screen.rsplit_once("hooks need review ") {
        if let Some((count, body)) = panel.split_once(' ') {
            let count = count.parse::<u32>().ok()?;
            let description = if count == 1 { "hook is new or changed." } else { "hooks are new or changed." };
            let body = body.replace(['›', '❯', '>'], " ").split_whitespace().collect::<Vec<_>>().join(" ");
            let expected = format!("{description} hooks can run outside the sandbox after you trust them. \
                1. review hooks 2. trust all and continue 3. continue without trusting (hooks won't run) enter confirm · esc skip");
            if count > 0 && body == expected && selected("1. review hooks") {
                return Some(Choice::CodexSkipUntrustedHooks);
            }
        }
    }
    None
}

pub(super) fn claude_composer(size: PtySize, transcript: &[u8], expected: Option<&str>) -> bool {
    let (Some(rows), Some(cols)) = (NonZeroU16::new(size.rows), NonZeroU16::new(size.cols)) else {
        return false;
    };
    if u32::from(rows.get()) * u32::from(cols.get()) > 512 * 512 {
        return false;
    }
    let mut terminal = vt100::Parser::new(rows, cols, 0);
    terminal.process(transcript);
    let screen = terminal.screen();
    let (row, col) = screen.cursor_position();
    if screen.hide_cursor() || !screen.cell(row, 0).is_some_and(|cell| matches!(cell.contents(), ">" | "❯"))
        || !screen.cell(row, 1).is_some_and(|cell| cell.contents().trim().is_empty())
    {
        return false;
    }
    match expected {
        None => col == 2,
        Some(text) => col as usize == text.len() + 2
            && screen.contents_between(row, 2, row, cols.get()).trim_end() == text,
    }
}

#[derive(Default)]
pub(super) struct Confirmation {
    observed: Option<(Choice, Instant)>,
    moved: bool,
    submitted: bool,
}

impl Confirmation {
    pub(super) fn input(&mut self, current: Option<Choice>, now: Instant) -> Option<&'static [u8]> {
        let Some(current) = current else {
            self.observed = None;
            return None;
        };
        if self.submitted {
            return None;
        }
        let since = match self.observed {
            Some((previous, since)) if previous == current => since,
            _ => {
                self.observed = Some((current, now));
                return None;
            }
        };
        if now.duration_since(since) < Duration::from_millis(250) {
            return None;
        }
        if current == Choice::ClaudeExit {
            if self.moved {
                return None;
            }
            self.moved = true;
            self.observed = None;
            return Some(b"\x1b[B");
        }
        self.submitted = true;
        if current == Choice::CodexSkipUntrustedHooks { Some(b"\x1b") } else { Some(b"\r") }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: PtySize = PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 };

    #[test]
    fn confirms_claude_only_after_the_affirmative_option_is_repainted() {
        let prompt = "Accessing workspace:\r\nC:\\managed\\worktree\r\n\r\n > No, exit\r\n   Yes, I trust this folder\r\n\r\n Enter to confirm · Esc to cancel";
        let exit = choice(SIZE, prompt);
        assert_eq!(exit, Some(Choice::ClaudeExit));
        let mut confirmation = Confirmation::default();
        let now = Instant::now();
        let elapsed = |ms| now + Duration::from_millis(ms);
        assert_eq!(confirmation.input(exit, now), None);
        assert_eq!(confirmation.input(exit, elapsed(300)), Some(b"\x1b[B".as_slice()));
        assert_eq!(confirmation.input(exit, elapsed(600)), None);
        assert_eq!(confirmation.input(exit, elapsed(900)), None);
        let trust = choice(SIZE, &prompt.replace("> No", "  No").replace("   Yes", " > Yes"));
        assert_eq!(trust, Some(Choice::ClaudeTrust));
        assert_eq!(confirmation.input(trust, elapsed(950)), None);
        assert_eq!(confirmation.input(trust, elapsed(1200)), Some(b"\r".as_slice()));
        assert_eq!(confirmation.input(trust, elapsed(1600)), None);
    }

    #[test]
    fn accepts_restricted_codex_but_never_saved_trust_or_an_approval_dialog() {
        let restricted = "Folder access\r\nConfig, hooks, and exec policies from untrusted folders stay disabled.\r\nOpening will not change saved\r\n trust.\r\n› 1. Open restricted\r\n 2. Back to Agent Command Center\r\n enter continue · esc back";
        assert_eq!(choice(SIZE, restricted), Some(Choice::CodexRestricted));
        assert_eq!(choice(SIZE, &restricted.replace("Open restricted", "Trust and continue")), None);
        assert_eq!(choice(SIZE, &restricted.replace("› 1.", "  1.").replace(" 2.", "› 2.")), None);
        assert_eq!(choice(SIZE, "Would you like to run the following command?\r\n› 1. Yes, proceed"), None);
    }

    #[test]
    fn skips_the_complete_fresh_codex_hook_picker_with_one_escape() {
        let frame = "Hooks need review\r\n2 hooks are new or changed.\r\nHooks can run outside the sandbox after you trust them.\r\n\r\n› 1. Review hooks\r\n  2. Trust all and continue\r\n  3. Continue without trusting (hooks won't run)\r\n\r\n enter confirm · esc skip";
        let skip = Some(Choice::CodexSkipUntrustedHooks);
        assert_eq!(choice(SIZE, frame), skip);
        assert_eq!(choice(SIZE, &frame.replace("2 hooks are", "1 hook is")), skip);
        for fragment in ["Hooks need review", "2 hooks are new or changed.", "Hooks can run outside the sandbox after you trust them.",
            "1. Review hooks", "2. Trust all and continue", "3. Continue without trusting", "hooks won't run", "esc skip"] {
            assert_eq!(choice(SIZE, &frame.replace(fragment, "")), None, "{fragment}");
        }
        assert_eq!(choice(SIZE, &frame.replace("esc skip", "esc back")), None);
        assert_eq!(choice(SIZE, &format!("{frame}\x1b[2J\x1b[H› Ask Codex anything")), None);
        let now = Instant::now();
        let mut confirmation = Confirmation::default();
        assert_eq!(confirmation.input(skip, now), None);
        assert_eq!(confirmation.input(None, now + Duration::from_millis(240)), None);
        assert_eq!(confirmation.input(skip, now + Duration::from_millis(300)), None);
        assert_eq!(confirmation.input(skip, now + Duration::from_millis(549)), None);
        assert_eq!(confirmation.input(skip, now + Duration::from_millis(550)), Some(b"\x1b".as_slice()));
        assert_eq!(confirmation.input(skip, now + Duration::from_secs(2)), None);
    }

    #[test]
    fn verifies_the_actual_claude_composer_instead_of_old_echo_or_a_paste_marker() {
        let empty = "> Try a command\x1b[1;3H";
        assert!(claude_composer(SIZE, empty.as_bytes(), None));
        let pointer = "Do task env CV_12345";
        let full = format!("> {pointer}");
        assert!(claude_composer(SIZE, full.as_bytes(), Some(pointer)));
        for frame in ["[Pasted text #1]", "> Do task env CV_123", "> Do task env CV_12345 extra",
            "> Do task env CV_12345\x1b[2J\x1b[H> Try a command\x1b[1;3H"]
        {
            assert!(!claude_composer(SIZE, frame.as_bytes(), Some(pointer)));
        }
    }
}
