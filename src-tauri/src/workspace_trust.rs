const MAX_VISIBLE_BYTES: usize = 16 * 1024;
const PROMPT_MARKERS: [&[u8]; 3] = [
    b"accessing workspace",
    b"yes, i trust this folder",
    b"enter to confirm",
];

#[derive(Default)]
pub struct WorkspaceTrustDetector {
    visible: Vec<u8>,
    ansi_state: AnsiState,
    matched: bool,
}

#[derive(Default)]
enum AnsiState {
    #[default]
    Text,
    Escape,
    Csi,
}

impl WorkspaceTrustDetector {
    pub fn push(&mut self, bytes: &[u8]) -> bool {
        if self.matched {
            return false;
        }

        for byte in bytes.iter().copied() {
            match self.ansi_state {
                AnsiState::Text if byte == 0x1b => {
                    self.ansi_state = AnsiState::Escape;
                }
                AnsiState::Text => {
                    self.visible.push(byte.to_ascii_lowercase());
                }
                AnsiState::Escape if byte == b'[' => {
                    self.ansi_state = AnsiState::Csi;
                }
                AnsiState::Escape => {
                    self.ansi_state = AnsiState::Text;
                }
                AnsiState::Csi if (0x40..=0x7e).contains(&byte) => {
                    if byte == b'C' {
                        self.visible.push(b' ');
                    }
                    self.ansi_state = AnsiState::Text;
                }
                AnsiState::Csi => {}
            }
        }

        if self.visible.len() > MAX_VISIBLE_BYTES {
            let overflow = self.visible.len() - MAX_VISIBLE_BYTES;
            self.visible.drain(..overflow);
        }

        self.matched = PROMPT_MARKERS
            .iter()
            .all(|marker| contains_bytes(&self.visible, marker));
        self.matched
    }

    #[cfg(test)]
    fn buffer_len(&self) -> usize {
        self.visible.len()
    }
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_fragmented_workspace_trust_prompt() {
        let mut detector = WorkspaceTrustDetector::default();

        assert!(!detector.push(b"\x1b[33mAccessing work"));
        assert!(!detector.push(b"space:\x1b[0m\r\n1. Yes, I trust "));
        assert!(detector.push(b"this folder\r\nEnter to confirm"));
    }

    #[test]
    fn detects_real_prompt_when_cursor_forward_encodes_spaces() {
        let mut detector = WorkspaceTrustDetector::default();
        let actual_shape = concat!(
            "\x1b[3;2HAccessing\x1b[1Cworkspace:",
            "\x1b[15;2H>\x1b[1C1.\x1b[1CYes,\x1b[1CI\x1b[1Ctrust",
            "\x1b[1Cthis\x1b[1Cfolder",
            "\x1b[18;2HEnter\x1b[1Cto\x1b[1Cconfirm"
        );

        assert!(detector.push(actual_shape.as_bytes()));
    }

    #[test]
    fn ignores_unrelated_trust_and_folder_text() {
        let mut detector = WorkspaceTrustDetector::default();

        assert!(!detector.push(
            b"I trust the folder layout, but this is ordinary assistant output"
        ));
    }

    #[test]
    fn detects_prompt_case_insensitively() {
        let mut detector = WorkspaceTrustDetector::default();

        assert!(detector.push(
            b"ACCESSING WORKSPACE\nYES, I TRUST THIS FOLDER\nENTER TO CONFIRM"
        ));
    }

    #[test]
    fn detector_buffer_stays_bounded() {
        let mut detector = WorkspaceTrustDetector::default();

        assert!(!detector.push(&vec![b'x'; 20 * 1024]));
        assert!(detector.buffer_len() <= 16 * 1024);
    }
}
