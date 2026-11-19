use std::io::Write as _;
use std::num::NonZeroU16;
use std::time::{Duration, Instant};

use anyhow::{ensure, Context, Result};
use portable_pty::PtySize;

use super::{
    codex_native_prompt_ready, interactive_runtime_prompt_blocked, last_codex_terminal_state,
    managed_approval_dialog, PermanentRunError, NATIVE_PROMPT_POST_RENDER_DELAY,
};

/// Codex redraws individual cells and can move the composer while a paste is
/// arriving. Track the rendered screen from an empty composer through this write.
pub(super) struct Confirmation<'a> {
    terminal: vt100::Parser<Activity>,
    fresh_echo: Option<vt100::Parser>,
    prompt: &'a str,
    marker: String,
    confirmed_since: Option<Instant>,
    output_bytes: usize,
    output_reads: usize,
    last_marker_count: Option<usize>,
    resizes: usize,
}

#[derive(Clone, Copy, PartialEq)]
enum State {
    Idle,
    Working,
    Done,
}

#[derive(Default)]
struct Activity {
    state: Option<State>,
    event: Option<State>,
    win32_input: bool,
}

impl vt100::Callbacks for Activity {
    fn unhandled_csi(
        &mut self,
        _: &mut vt100::Screen,
        first: Option<u8>,
        second: Option<u8>,
        params: &[&[u16]],
        action: char,
    ) {
        if first == Some(b'?') && second.is_none() && params.contains(&&[9001][..]) {
            match action {
                'h' => self.win32_input = true,
                'l' => self.win32_input = false,
                _ => {}
            }
        }
    }

    fn unhandled_osc(&mut self, _: &mut vt100::Screen, params: &[&[u8]]) {
        let [b"7501", fields] = params else {
            return;
        };
        let mut fields = fields.split(|byte| *byte == b':');
        let state = match fields.next() {
            Some(b"state=working") => State::Working,
            Some(b"state=idle") => State::Idle,
            Some(b"state=done") => State::Done,
            _ => return,
        };
        if fields.any(|field| field == b"app=codex") {
            self.state = Some(state);
            self.event = Some(state);
        }
    }
}

impl<'a> Confirmation<'a> {
    pub(super) fn bracketed_paste(&self) -> Vec<u8> {
        if !cfg!(windows) || !self.terminal.callbacks().win32_input {
            return super::utf8_bracketed_paste(self.prompt);
        }
        // ConPTY's plain-text fallback synthesizes keyboard-layout events.
        // Codex drops characters such as →, ° and × on that path. Neutral,
        // modifier-free Unicode key-down records carry the original UTF-16
        // units, including adjacent surrogate halves, inside one paste.
        let mut paste = b"\x1b[200~".to_vec();
        for ch in self.prompt.chars() {
            if ch.is_ascii() {
                paste.push(ch as u8);
            } else {
                for unit in ch.encode_utf16(&mut [0; 2]) {
                    write!(paste, "\x1b[0;0;{unit};1;0;1_").expect("writing to Vec cannot fail");
                }
            }
        }
        paste.extend_from_slice(b"\x1b[201~");
        paste
    }

    pub(super) fn begin(size: PtySize, baseline: &[u8], prompt: &'a str) -> Result<Self> {
        let (rows, cols) = dimensions(size)?;
        let mut terminal = vt100::Parser::new_with_callbacks(rows, cols, 0, Activity::default());
        terminal.process(baseline);
        terminal.callbacks_mut().event = None;
        let screen = terminal.screen();
        let available = screen_available(screen)?;
        ensure!(
            available
                && terminal.callbacks().state != Some(State::Working)
                && empty_composer(screen),
            "Codex 原生输入框未确认空闲，托管任务未粘贴"
        );
        ensure!(!prompt.trim().is_empty(), "Codex 托管任务为空");
        ensure!(
            !prompt
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\t')),
            "Codex 托管任务含有终端控制字符，未粘贴"
        );
        Ok(Self {
            terminal,
            fresh_echo: None,
            prompt,
            marker: format!("[Pasted Content {} chars]", prompt.chars().count()),
            confirmed_since: None,
            output_bytes: 0,
            output_reads: 0,
            last_marker_count: None,
            resizes: 0,
        })
    }

    pub(super) fn observe(&mut self, size: PtySize, bytes: &[u8], now: Instant) -> Result<bool> {
        let (rows, cols) = dimensions(size)?;
        if self.terminal.screen().size() != (rows, cols) {
            self.resizes += 1;
            self.terminal.set_size(rows, cols);
            if let Some(proof) = self.fresh_echo.as_mut() {
                proof.set_size(rows, cols);
            }
            self.confirmed_since = None;
        }
        if !bytes.is_empty() {
            self.output_bytes = self.output_bytes.saturating_add(bytes.len());
            self.output_reads += 1;
            // Stop exactly at OSC terminators. A single PTY read can contain
            // working, idle and a fresh redraw, or the sequences can be split.
            for part in bytes.split_inclusive(|byte| matches!(byte, 7 | b'\\')) {
                let previous = self.terminal.callbacks().state;
                self.terminal.process(part);
                match self.terminal.callbacks_mut().event.take() {
                    Some(State::Working) => {
                        self.confirmed_since = None;
                        self.fresh_echo = None;
                    }
                    Some(State::Done) => {
                        return Err(PermanentRunError(
                            "Codex 在粘贴期间结束了其他轮次，托管任务未提交".into(),
                        )
                        .into())
                    }
                    Some(State::Idle) if previous == Some(State::Working) => {
                        // Keep geometry/caret, but no old draft text in the
                        // independent proof. Idle, cursor motion and footer
                        // changes alone cannot re-confirm an earlier paste.
                        self.fresh_echo = Some(empty_evidence_screen(self.terminal.screen()));
                    }
                    _ => {
                        if let Some(proof) = self.fresh_echo.as_mut() {
                            proof.process(part);
                        }
                    }
                }
            }
            let screen = self.terminal.screen();
            if let Some(count) = visible_marker_count(screen) {
                self.last_marker_count = Some(count);
            }
            let available = screen_available(screen)?;
            let matches = |screen: &vt100::Screen| {
                !screen.hide_cursor()
                    && (composer_matches(screen, &self.marker)
                        || composer_matches(screen, self.prompt))
            };
            if available
                && self.terminal.callbacks().state != Some(State::Working)
                && matches(screen)
                && self
                    .fresh_echo
                    .as_ref()
                    .is_none_or(|proof| matches(proof.screen()))
            {
                self.confirmed_since.get_or_insert(now);
            } else {
                self.confirmed_since = None;
            }
        }
        // Keep polling during the settle interval: an approval or a clear can
        // replace an otherwise correct echo before Enter would be written.
        Ok(self
            .confirmed_since
            .is_some_and(|since| now.duration_since(since) >= NATIVE_PROMPT_POST_RENDER_DELAY))
    }

    pub(super) fn wait_reason(&self) -> String {
        let reason = if self.terminal.callbacks().state == Some(State::Working) {
            "Codex 工作状态尚未结束"
        } else if self.fresh_echo.is_some() {
            "Codex 恢复空闲后未重新显示本轮粘贴"
        } else {
            "未观察到完整的本轮粘贴"
        };
        let screen = self.terminal.screen();
        let (rows, cols) = screen.size();
        let (row, col) = screen.cursor_position();
        let anchor = (0..=row).rev().find(|r| composer_row(screen, *r));
        let field = match anchor {
            None => "无起始标记",
            Some(_) if empty_composer(screen) => "空框",
            Some(_) if visible_marker_count(screen).is_some() => "含折叠标记",
            Some(_) => "文本",
        };
        // Persist only dimensions, counts and categorical evidence. Prompt,
        // terminal text and CLI credentials must never enter task errors.
        let marker = self
            .last_marker_count
            .map_or_else(|| "无".into(), |n| n.to_string());
        let start = anchor.map_or_else(|| "无".into(), |r| (r + 1).to_string());
        let trailing = screen
            .contents_between(row, col, row, cols.get())
            .trim()
            .chars()
            .count();
        format!("{reason}，终端 {cols}×{rows}（调整 {} 次），期望 {} 字符，回显 {} 字节/{} 次，可见折叠计数 {marker}，输入框 {field}（起始行 {start}），光标 {} 行 {} 列，光标右侧 {trailing} 字符{}",
            self.resizes, self.prompt.chars().count(), self.output_bytes, self.output_reads, row + 1, col + 1,
            if screen.hide_cursor() { "，已隐藏" } else { "" })
    }
}

fn visible_marker_count(screen: &vt100::Screen) -> Option<usize> {
    let visible = screen
        .rows(0, screen.size().1.get())
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    let words = visible.split_whitespace().collect::<Vec<_>>().join(" ");
    let (_, rest) = words.rsplit_once("[pasted content ")?;
    let (number, _) = rest.split_once(" chars]")?;
    number.parse().ok()
}

fn empty_evidence_screen(screen: &vt100::Screen) -> vt100::Parser {
    let (rows, cols) = screen.size();
    let (row, col) = screen.cursor_position();
    let mut proof = vt100::Parser::new(rows, cols, 0);
    if let Some(start) = (0..=row).rev().find(|r| composer_row(screen, *r)) {
        proof.process(format!("\x1b[{};1H› ", start + 1).as_bytes());
    }
    proof.process(format!("\x1b[{};{}H", row + 1, col + 1).as_bytes());
    proof
}

#[derive(Default)]
pub(super) struct Startup {
    idle_since: Option<Instant>,
    state_end: Option<usize>,
    last_frame: StartupFrame,
}

#[derive(Default)]
struct StartupFrame {
    state: Option<bool>,
    empty_composer: bool,
    protected_view: Option<&'static str>,
    ready: bool,
}

impl Startup {
    pub(super) fn observe(&mut self, size: PtySize, transcript: &str, now: Instant) -> Result<()> {
        let state = last_codex_terminal_state(transcript);
        let state_end = state.map(|(_, end)| end);
        self.last_frame = startup_frame(size, transcript, state.map(|(idle, _)| idle))?;
        let ready = self.last_frame.ready;
        // The first idle precedes a measured 280–313 ms startup activity
        // pulse. A new OSC epoch or a nonempty/blocked frame restarts settling.
        if self.state_end != state_end || !ready {
            self.idle_since = None;
        }
        self.state_end = state_end;
        if ready {
            self.idle_since.get_or_insert(now);
        }
        Ok(())
    }

    pub(super) fn ready(&self, now: Instant) -> bool {
        self.idle_since
            .is_some_and(|since| now.duration_since(since) >= Duration::from_millis(500))
    }

    pub(super) fn timeout(&self, title: &str) -> anyhow::Error {
        if let Some(view) = self.last_frame.protected_view {
            return PermanentRunError(format!("{title} 等待{view}，已停止托管，任务未提交")).into();
        }
        let state = match self.last_frame.state {
            Some(true) => "空闲",
            Some(false) => "工作中",
            None => "未收到",
        };
        let composer = if self.last_frame.empty_composer { "已确认空输入框" } else { "未确认空输入框" };
        anyhow::anyhow!("{title} 原生会话未就绪，任务未提交（状态信号：{state}，{composer}）")
    }
}

fn startup_frame(size: PtySize, transcript: &str, state: Option<bool>) -> Result<StartupFrame> {
    let (rows, cols) = dimensions(size)?;
    let mut terminal = vt100::Parser::new(rows, cols, 0);
    terminal.process(transcript.as_bytes());
    let screen = terminal.screen();
    let empty_composer = empty_composer(screen);
    let visible = screen.contents();
    let words = visible.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_lowercase();
    let protected_view = if !empty_composer && (words.contains("do you trust the contents")
        || (words.contains("folder access") && words.contains("trust and continue"))) {
        Some("工作区信任确认")
    } else if ["sign in to continue", "sign in with chatgpt", "not logged in", "authentication required"].iter().any(|marker| words.contains(marker)) {
        Some("登录")
    } else if managed_approval_dialog(&words) {
        Some("人工审批")
    } else {
        None
    };
    // OSC is terminal state, not painted text. ConPTY can deliver it just
    // before a full repaint or in a different chunk from the composer.
    let ready = empty_composer && protected_view.is_none() && matches!(screen_available(screen), Ok(true))
        && state.unwrap_or_else(|| codex_native_prompt_ready(&visible));
    Ok(StartupFrame { state, empty_composer, protected_view, ready })
}

fn empty_composer(screen: &vt100::Screen) -> bool {
    let (row, col) = screen.cursor_position();
    // The caret is on the first editable cell. Current Codex displays its
    // placeholder there in dim text, including localized/rotating hints.
    let body = screen.contents_between(row, 2, row, screen.size().1.get());
    !screen.hide_cursor()
        && col == 2
        && composer_row(screen, row)
        && (body.trim().is_empty() || screen.cell(row, col).is_some_and(|cell| cell.dim()))
}

pub(super) fn update_menu_is_selected(size: PtySize, bytes: &[u8]) -> Result<bool> {
    let (rows, cols) = dimensions(size)?;
    let mut terminal = vt100::Parser::new(rows, cols, 0);
    terminal.process(bytes);
    let lines = terminal.screen().rows(0, cols.get()).collect::<Vec<_>>();
    let selected = lines
        .iter()
        .rev()
        .find_map(|line| line.trim().strip_prefix(['›', '❯']));
    let words = lines
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    Ok(selected.is_some_and(|choice| {
        choice
            .trim_start()
            .to_ascii_lowercase()
            .starts_with("1. update now")
    }) && words.contains("2. skip")
        && super::codex_native_update_prompt(&words)
        && !managed_approval_dialog(&words))
}

fn dimensions(size: PtySize) -> Result<(NonZeroU16, NonZeroU16)> {
    // Same screen budget as SessionManager's runtime prompt projection. A
    // clipped view could place the caret in a different row, so fail closed.
    ensure!(
        u32::from(size.rows) * u32::from(size.cols) <= 512 * 512,
        "Codex 终端尺寸超出可验证范围"
    );
    Ok((
        NonZeroU16::new(size.rows).context("Codex 终端行数为零")?,
        NonZeroU16::new(size.cols).context("Codex 终端列数为零")?,
    ))
}

fn composer_row(screen: &vt100::Screen, row: u16) -> bool {
    screen
        .cell(row, 0)
        .is_some_and(|cell| matches!(cell.contents(), "›" | "❯"))
        && screen
            .cell(row, 1)
            .is_some_and(|cell| cell.contents().trim().is_empty())
}

fn screen_available(screen: &vt100::Screen) -> Result<bool> {
    let visible = screen
        .rows(0, screen.size().1.get())
        .map(|row| row.trim_end().to_owned())
        .collect::<Vec<_>>()
        .join("\n");
    let words = visible
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    if managed_approval_dialog(&words) {
        return Err(PermanentRunError("原生终端显示审批面板，已停止提交托管任务".into()).into());
    }
    // Codex uses Enter to choose an autocomplete item while its composer
    // popup is open, even if the entire pasted text is visible below it.
    if visible.lines().any(|line| {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_lowercase();
        line.contains("enter insert") && line.contains("esc close")
    }) {
        return Ok(false);
    }
    let blocked = interactive_runtime_prompt_blocked(&visible);
    let busy = super::interactive_runtime_prompt_busy(&visible);
    if (blocked && !busy)
        || [
            "sign in to continue",
            "sign in with chatgpt",
            "authentication required",
            "do you trust the contents",
            "waiting for approval",
            "enter to confirm",
        ]
        .iter()
        .any(|marker| words.contains(marker))
    {
        return Err(PermanentRunError(
            "Codex 原生终端正在工作或需要登录、授权，已停止提交托管任务".into(),
        )
        .into());
    }
    if busy
        && (words.contains("• working")
            || words.contains("working (")
            || words.contains("enter to steer")
            || words.contains("enter steers"))
    {
        return Err(
            PermanentRunError("Codex 已显示正在执行的模型轮次，托管任务未提交".into()).into(),
        );
    }
    Ok(!busy)
}

/// Compare the *entire* input up to the visible caret. A footer, history row,
/// partial prefix, stale marker or a different paste length cannot qualify.
fn composer_matches(screen: &vt100::Screen, expected: &str) -> bool {
    let (caret_row, caret_col) = screen.cursor_position();
    let width = screen.size().1.get();
    if caret_col < 2
        || !screen
            .contents_between(caret_row, caret_col, caret_row, width)
            .trim()
            .is_empty()
    {
        return false;
    }
    let Some(first_row) = (0..=caret_row).rev().find(|row| composer_row(screen, *row)) else {
        return false;
    };
    if screen
        .cell(first_row, 2)
        .is_some_and(|cell| cell.has_contents() && cell.dim())
    {
        return false;
    }
    // The CLI indents continuation rows by two cells and word-wraps to the
    // pane. Only a physical row boundary may stand in for a space or newline.
    let mut remaining = expected;
    for row in first_row..=caret_row {
        if row != first_row && !screen.contents_between(row, 0, row, 2).trim().is_empty() {
            return false;
        }
        let last = row == caret_row;
        let text = screen.contents_between(row, 2, row, if last { caret_col } else { width });
        if last {
            return (!remaining.is_empty() || expected.ends_with('\n')) && remaining == text;
        }
        let Some(rest) = remaining.strip_prefix(text.trim_end_matches(' ')) else {
            return false;
        };
        remaining = rest
            .strip_prefix('\n')
            .unwrap_or_else(|| rest.trim_start_matches(' '));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn startup_ready(size: PtySize, transcript: &str) -> Result<bool> {
        Ok(startup_frame(size, transcript, last_codex_terminal_state(transcript).map(|(idle, _)| idle))?.ready)
    }

    #[test]
    fn startup_timeout_retries_missing_readiness_and_stops_at_visible_trust() {
        let now = Instant::now();
        let mut startup = Startup::default();
        startup.observe(size(80), "OpenAI Codex (v0.169.0)", now).unwrap();
        assert!(!startup.ready(now));
        assert!(startup.timeout("Codex").downcast_ref::<PermanentRunError>().is_none());

        let trust = "\x1b[2J\x1b[HFolder access\r\nDo you trust these files?\r\n› 1. Trust and continue\r\n2. Back to Agent Command Center";
        startup.observe(size(80), trust, now).unwrap();
        assert!(!startup.ready(now + Duration::from_secs(2)));
        let error = startup.timeout("Codex");
        assert!(error.downcast_ref::<PermanentRunError>().is_some());
        assert!(error.to_string().contains("工作区信任确认"));
        assert!(!error.to_string().contains("Trust and continue"));
    }

    fn size(cols: u16) -> PtySize {
        PtySize {
            rows: 24,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        }
    }

    fn empty(row: u16) -> String {
        format!("\x1b[{row};1H\x1b[1m›\x1b[22m \x1b[2mAsk Codex to do anything\x1b[22m\x1b[{row};3H\x1b[?25h")
    }

    fn sample_prompt() -> String {
        format!(
            "你是协作项目中的成员，负责验证贪吃蛇任务中文粘贴回显。\n{}本轮探针结束。",
            "多行任务一，请仅核对当前工作目录中移动、得分与碰撞的显示；emoji🐍，中文分屏验证。\n"
                .repeat(40)
        )
    }

    #[test]
    fn codex_unicode_input_requires_negotiated_win32_mode_and_preserves_surrogate_pairs() {
        let prompt = "A→°×🐍\nB";
        let fallback = super::super::utf8_bracketed_paste(prompt);
        let win32 = b"\x1b[200~A\x1b[0;0;8594;1;0;1_\x1b[0;0;176;1;0;1_\x1b[0;0;215;1;0;1_\x1b[0;0;55357;1;0;1_\x1b[0;0;56333;1;0;1_\nB\x1b[201~";
        for (mode, enabled) in [
            ("", false),
            ("\x1b[?9001h", true),
            ("\x1b[?9001;1004h", true),
            ("\x1b[?9001h\x1b[?9001l", false),
        ] {
            let baseline = format!("{mode}{}", empty(3));
            let receipt = Confirmation::begin(size(80), baseline.as_bytes(), prompt).unwrap();
            assert_eq!(
                receipt.bracketed_paste(),
                if cfg!(windows) && enabled {
                    &win32[..]
                } else {
                    &fallback[..]
                }
            );
        }
    }

    #[test]
    fn codex_timeout_diagnostic_only_reports_counts_and_categories() {
        let start = Instant::now();
        let secret = "PRIVATE_TEST_TEXT_仅用于检查日志";
        let mut echo = Confirmation::begin(size(62), empty(9).as_bytes(), secret).unwrap();
        let private_output = "\x1b[2;1HPRIVATE_CLI_TEXT\x1b[9;3H[Pasted Content 321 chars]\x1b[K";
        echo.observe(size(62), private_output.as_bytes(), start)
            .unwrap();
        let summary = echo.wait_reason();
        assert!(
            summary.contains("终端 62×24")
                && summary.contains("可见折叠计数 321")
                && summary.contains("/1 次")
        );
        assert!(
            !summary.contains("PRIVATE")
                && !summary.contains("仅用于")
                && !summary.contains("Pasted Content")
        );
    }

    // Codex 0.169.0 emitted this sequence while starting a new thread, before
    // any input: idle, then working 312 ms later, then idle after another 10 ms.
    const IDLE: &str = "\x1b]7501;state=idle:app=codex\x1b\\";
    const WORKING: &str = "\x1b]7501;state=working:app=codex\x1b\\";

    #[test]
    fn codex_startup_pulse_restarts_the_continuously_observed_idle_interval() {
        let now = Instant::now();
        let ms = |n| now + Duration::from_millis(n);
        let initial = format!("{}{}", empty(9), IDLE);
        let pulse = format!("{initial}{WORKING}");
        for combined in [false, true] {
            let mut startup = Startup::default();
            startup.observe(size(62), &initial, now).unwrap();
            assert!(!startup.ready(ms(311)));
            if !combined {
                startup.observe(size(62), &pulse, ms(312)).unwrap();
                assert!(!startup.ready(ms(321)));
            }
            startup
                .observe(size(62), &format!("{pulse}{IDLE}"), ms(322))
                .unwrap();
            assert!(!startup.ready(ms(500)));
            assert!(startup.ready(ms(822)));
            startup
                .observe(
                    size(62),
                    &format!("{pulse}{IDLE}\x1b[9;3Hdraft\x1b[K"),
                    ms(830),
                )
                .unwrap();
            assert!(!startup.ready(ms(1400)));
        }
    }

    #[test]
    fn codex_post_paste_working_waits_for_idle_and_for_the_composer_to_repaint() {
        let prompt = sample_prompt();
        let start = Instant::now();
        let settle = NATIVE_PROMPT_POST_RENDER_DELAY;
        let baseline = format!("{}{}", empty(9), IDLE);
        let marker = b"\x1b[9;3H\x1b[38;5;6m[Pasted Content 1795 chars]\x1b[m\x1b[K";
        let mut echo = Confirmation::begin(size(62), baseline.as_bytes(), &prompt).unwrap();
        assert!(!echo.observe(size(62), marker, start).unwrap());
        for byte in WORKING.as_bytes().chunks(1) {
            assert!(!echo.observe(size(62), byte, start).unwrap());
        }
        assert!(!echo.observe(size(62), marker, start + settle).unwrap());
        assert!(!echo.observe(size(62), b"", start + 2 * settle).unwrap());
        assert!(!echo
            .observe(size(62), IDLE.as_bytes(), start + 2 * settle)
            .unwrap());
        assert!(!echo
            .observe(
                size(62),
                b"\x1b[12;1Hunrelated footer\x1b[9;30H",
                start + 3 * settle
            )
            .unwrap());
        assert!(!echo.observe(size(62), b"", start + 4 * settle).unwrap());
        assert!(!echo.observe(size(62), marker, start + 4 * settle).unwrap());
        assert!(echo.observe(size(62), b"", start + 5 * settle).unwrap());

        let mut together = Confirmation::begin(size(62), baseline.as_bytes(), &prompt).unwrap();
        let mut combined = format!("{WORKING}{IDLE}").into_bytes();
        combined.extend_from_slice(marker);
        assert!(!together.observe(size(62), &combined, start).unwrap());
        assert!(together.observe(size(62), b"", start + settle).unwrap());
    }

    #[test]
    fn codex_busy_does_not_dismiss_approval_or_an_actual_model_turn() {
        let prompt = sample_prompt();
        let start = Instant::now();
        for stop in [
            "\x1b]7501;state=done:app=codex\x07",
            "\x1b[H\x1b[2J• Working (2s • esc to interrupt)",
            "\x1b[H\x1b[2JWould you like to run the following command?\r\n1. Yes, proceed (y)\r\n2. No, and tell Codex what to do differently (esc)",
            "\x1b[H\x1b[2JSign in with ChatGPT",
        ] {
            let mut echo = Confirmation::begin(size(62), empty(9).as_bytes(), &prompt).unwrap();
            assert!(!echo.observe(size(62), WORKING.as_bytes(), start).unwrap());
            assert!(echo.observe(size(62), stop.as_bytes(), start).is_err(), "{stop:?}");
        }
    }

    #[test]
    fn codex_update_shortcut_requires_the_current_selection() {
        let update =
            "› 1. Update now\r\n2. Skip\r\n3. Skip until next version\r\nenter continue · esc skip";
        assert!(update_menu_is_selected(size(80), update.as_bytes()).unwrap());
        assert!(
            !update_menu_is_selected(size(80), format!("{update}{}", empty(7)).as_bytes()).unwrap()
        );
        let replaced = format!("{update}\x1b[2J\x1b[HSign in with ChatGPT");
        assert!(!update_menu_is_selected(size(80), replaced.as_bytes()).unwrap());
    }

    // Verbatim post-paste redraw from Codex 0.169.0 in a 62-column Windows
    // ConPTY. It only updates cells at the already-positioned input cursor.
    const PASTE_1795: &[u8] = b"\x1b[?2026h\x1b[?25l\x1b[?2026l\x1b[38;5;6m[Pasted Content 1795 chars]\x1b[m\x1b[12;3H \x1b[1C   \x1b[1C      \x1b[1C \x1b[1C \x1b[1C   \x1b[1C\x1b[9X\x1b[9;30H\x1b[?25h";

    #[test]
    fn codex_conpty_paste_accepts_the_current_count_across_chunk_boundaries() {
        let prompt = sample_prompt();
        assert_eq!((prompt.chars().count(), prompt.len()), (1795, 4943));
        let start = Instant::now();
        for split in 0..=PASTE_1795.len() {
            let mut echo = Confirmation::begin(size(62), empty(9).as_bytes(), &prompt).unwrap();
            assert!(!echo.observe(size(62), &PASTE_1795[..split], start).unwrap());
            assert!(!echo.observe(size(62), &PASTE_1795[split..], start).unwrap());
            assert!(
                echo.observe(size(62), b"", start + NATIVE_PROMPT_POST_RENDER_DELAY)
                    .unwrap(),
                "split {split}"
            );
        }
    }

    #[test]
    fn codex_conpty_wraps_the_marker_and_incrementally_paints_short_chinese() {
        let prompt = sample_prompt();
        let start = Instant::now();
        let mut echo = Confirmation::begin(size(28), empty(12).as_bytes(), &prompt).unwrap();
        let wrapped = "\x1b[12;3H\x1b[38;5;6m[Pasted Content 1795      \x1b[m\r\n  \x1b[38;5;6mchars]\x1b[K\x1b[m\x1b[13;9H\x1b[?25h";
        assert!(!echo.observe(size(28), wrapped.as_bytes(), start).unwrap());
        assert!(echo
            .observe(size(28), b"", start + NATIVE_PROMPT_POST_RENDER_DELAY)
            .unwrap());

        let mut short =
            Confirmation::begin(size(52), empty(10).as_bytes(), "探测贪吃蛇输入回显").unwrap();
        let prefix = "探\x1b[K\x1b[10;5H";
        assert!(!short.observe(size(52), prefix.as_bytes(), start).unwrap());
        assert!(!short
            .observe(size(52), b"", start + NATIVE_PROMPT_POST_RENDER_DELAY)
            .unwrap());
        let rest = "\x1b[?2026h\x1b[?2026l测贪吃蛇输入回显".as_bytes();
        for byte in rest.chunks(1) {
            assert!(!short.observe(size(52), byte, start).unwrap());
        }
        assert!(short
            .observe(size(52), b"", start + NATIVE_PROMPT_POST_RENDER_DELAY)
            .unwrap());
    }

    #[test]
    fn codex_narrow_terminal_requires_the_entire_task_file_instruction() {
        let prompt = super::super::MANAGED_CODEX_TASK_POINTER;
        let start = Instant::now();
        let settle = NATIVE_PROMPT_POST_RENDER_DELAY;
        let mut echo = Confirmation::begin(size(25), empty(9).as_bytes(), prompt).unwrap();
        let (prefix, suffix) = prompt.as_bytes().split_at(prompt.len() - 3);
        assert!(!echo.observe(size(25), &[prefix, b"\x1b[K"].concat(), start).unwrap());
        assert!(!echo.observe(size(25), b"", start + settle).unwrap());
        assert!(!echo.observe(size(25), suffix, start + settle).unwrap());
        assert!(echo.observe(size(25), b"", start + 2 * settle).unwrap());
    }

    #[test]
    fn codex_does_not_submit_a_matching_echo_under_an_autocomplete_popup() {
        let prompt = "Do task: $env:CV_TASK";
        let start = Instant::now();
        let settle = NATIVE_PROMPT_POST_RENDER_DELAY;
        let mut echo = Confirmation::begin(size(50), empty(9).as_bytes(), prompt).unwrap();
        let popup = format!("\x1b[7;3Henter insert · esc close\x1b[9;3H{prompt}\x1b[K");
        assert!(!echo.observe(size(50), popup.as_bytes(), start).unwrap());
        assert!(!echo.observe(size(50), b"", start + settle).unwrap());
        let dismiss = format!("\x1b[7;1H\x1b[2K\x1b[9;{}H", prompt.len() + 3);
        assert!(!echo.observe(size(50), dismiss.as_bytes(), start + settle).unwrap());
        assert!(echo.observe(size(50), b"", start + 2 * settle).unwrap());
    }

    #[test]
    fn codex_paste_rejects_history_foreign_output_wrong_counts_and_erased_input() {
        let prompt = sample_prompt();
        let stale = "\x1b[2;1H› [Pasted Content 1795 chars]";
        assert!(Confirmation::begin(size(62), stale.as_bytes(), &prompt).is_err());
        let baseline = format!("{stale}{}", empty(9));
        let start = Instant::now();
        for wrong in [
            "\x1b[?25h", // no change to this composer
            "\x1b[2;1H› [Pasted Content 1795 chars]\x1b[9;3H",
            "\x1b[11;1HLog: [Pasted Content 1795 chars]\x1b[9;3H",
            "\x1b[9;3H[Pasted Content 4943 chars]\x1b[K", // byte count
            "\x1b[9;3H[Pasted text #1]\x1b[K",            // Claude marker
            "\x1b[9;3H[Pasted Content 1795 chars] other input\x1b[9;30H",
        ] {
            let mut echo = Confirmation::begin(size(62), baseline.as_bytes(), &prompt).unwrap();
            assert!(!echo.observe(size(62), wrong.as_bytes(), start).unwrap());
            assert!(!echo
                .observe(size(62), b"", start + NATIVE_PROMPT_POST_RENDER_DELAY)
                .unwrap());
        }
        let mut echo = Confirmation::begin(size(62), baseline.as_bytes(), &prompt).unwrap();
        assert!(!echo.observe(size(62), PASTE_1795, start).unwrap());
        assert!(!echo
            .observe(
                size(62),
                empty(9).as_bytes(),
                start + NATIVE_PROMPT_POST_RENDER_DELAY
            )
            .unwrap());
    }

    #[test]
    fn codex_approval_or_login_replacing_a_confirmed_paste_prevents_enter() {
        let prompt = sample_prompt();
        let start = Instant::now();
        for dialog in [
            "\x1b[H\x1b[2JWould you like to run the following command?\r\n1. Yes, proceed (y)\r\n2. No, and tell Codex what to do differently (esc)",
            "\x1b[HSign in with ChatGPT",
            "\x1b[HDo you trust the contents of this directory?",
        ] {
            let mut echo = Confirmation::begin(size(62), empty(9).as_bytes(), &prompt).unwrap();
            assert!(!echo.observe(size(62), PASTE_1795, start).unwrap());
            assert!(echo.observe(size(62), dialog.as_bytes(), start + NATIVE_PROMPT_POST_RENDER_DELAY).is_err());
        }
    }

    #[test]
    fn codex_managed_startup_requires_an_empty_composer_and_native_idle_after_repaint() {
        let frame = format!("\x1b[HOpenAI Codex (v0.169.0){}", empty(3));
        let idle = "\x1b]7501;state=idle:app=codex\x07";
        let working = "\x1b]7501;state=working:app=codex\x1b\\";
        assert!(!startup_ready(size(80), &frame).unwrap());
        assert!(startup_ready(size(80), &format!("{idle}{frame}")).unwrap());
        assert!(!startup_ready(size(80), &format!("{idle}{working}{frame}")).unwrap());
        assert!(!startup_ready(size(80), &format!("{idle}{frame}\x1b[3;3Ha draft\x1b[K")).unwrap());
    }

    #[test]
    fn codex_resize_waits_for_a_fresh_composer_and_never_counts_placeholder_text() {
        let prompt = sample_prompt();
        let start = Instant::now();
        let mut echo = Confirmation::begin(size(62), empty(9).as_bytes(), &prompt).unwrap();
        assert!(!echo.observe(size(62), PASTE_1795, start).unwrap());
        let later = start + NATIVE_PROMPT_POST_RENDER_DELAY;
        assert!(!echo.observe(size(42), b"", later).unwrap());
        assert!(!echo
            .observe(size(42), b"", later + NATIVE_PROMPT_POST_RENDER_DELAY)
            .unwrap());
        let fresh = "\x1b[9;1H› [Pasted Content 1795 chars]\x1b[K";
        assert!(!echo.observe(size(42), fresh.as_bytes(), later).unwrap());
        assert!(echo
            .observe(size(42), b"", later + NATIVE_PROMPT_POST_RENDER_DELAY)
            .unwrap());

        let mut hint =
            Confirmation::begin(size(62), empty(9).as_bytes(), "Ask Codex to do anything").unwrap();
        assert!(!hint.observe(size(62), b"\x1b[9;27H", start).unwrap());
        assert!(!hint.observe(size(62), b"", later).unwrap());
        assert!(Confirmation::begin(size(62), empty(9).as_bytes(), "log\x1b[0m").is_err());
    }
}
