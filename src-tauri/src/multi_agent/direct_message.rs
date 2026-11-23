use super::AgentRuntimeKind;

#[derive(Debug, PartialEq, Eq)]
pub enum DirectMessageRequest {
    Broadcast,
    Directed { sender: String, recipient: String },
}

fn message_instruction(message: &str) -> Option<&str> {
    let mut instruction = message.trim();
    while instruction.starts_with('@') {
        let end = instruction.find(char::is_whitespace)?;
        instruction = instruction[end..].trim_start();
    }
    Some(instruction)
}

pub(crate) fn self_introduction_requested(message: &str) -> bool {
    let Some(instruction) = message_instruction(message) else { return false; };
    let instruction = outside_quotes(instruction);
    ["自我介绍", "介绍自己", "互相介绍", "相互介绍", "彼此介绍", "互相认识", "相互认识"]
        .iter().any(|phrase| instruction.contains(phrase))
}

/// Only the instruction is classified; quoted message content may itself discuss development.
pub fn direct_message_request(message: &str) -> Option<DirectMessageRequest> {
    let instruction = message_instruction(message)?;
    let normalized = instruction.to_ascii_lowercase();
    if let Some((sender, rest)) = normalized.split_once(['给', '向', '对']) {
        let action = [
            "发一句",
            "发句",
            "说一句",
            "说句",
            "发一条消息",
            "发条消息",
            "发个消息",
            "发消息",
            "发送消息",
            "发送",
            "打个招呼",
            "打招呼",
            "问候",
        ]
        .iter()
        .filter_map(|verb| rest.find(verb).map(|position| (position, *verb)))
        .min_by_key(|(position, _)| *position);
        if let Some((position, _)) = action {
            // A second explicit action is still a work plan, not quoted message content.
            let unquoted_body = outside_quotes(&rest[position..]);
            if ["然后", "接着", "并且"].iter().any(|connector| {
                unquoted_body
                    .split_once(connector)
                    .is_some_and(|(_, next_instruction)| {
                        is_development_instruction(next_instruction)
                    })
            }) {
                return None;
            }
            // A later sentence such as “收到消息后向原发送方回复” is
            // a reply rule, not the sender/recipient clause of this request.
            let crosses_sentence = sender.contains(['。', '；', '\n'])
                || rest[..position].contains(['。', '；', '\n']);
            if !crosses_sentence && !is_development_instruction(&format!("{sender}{}", &rest[..position])) {
                let sender = sender
                    .trim()
                    .trim_start_matches("请")
                    .trim_start_matches("让")
                    .trim();
                let recipient = rest[..position].trim();
                if !sender.is_empty() && !recipient.is_empty() {
                    return Some(DirectMessageRequest::Directed {
                        sender: sender.into(),
                        recipient: recipient.into(),
                    });
                }
            }
        }
    }
    (!is_development_instruction(&normalized)
        && (self_introduction_requested(instruction) || [
            "聊天",
            "聊下",
            "聊一下",
            "聊一聊",
            "聊聊",
            "聊个天",
            "通信",
            "发消息",
            "问候",
            "打招呼",
            "打个招呼",
            "message",
        ]
        .iter()
        .any(|word| normalized.contains(word))))
    .then_some(DirectMessageRequest::Broadcast)
}

fn outside_quotes(mut text: &str) -> String {
    let mut unquoted = String::new();
    while let Some(character) = text.chars().next() {
        text = &text[character.len_utf8()..];
        let closing = match character {
            '「' => Some('」'),
            '“' => Some('”'),
            '"' => Some('"'),
            '\'' => Some('\''),
            _ => None,
        };
        if let Some(end) = closing.and_then(|closing| {
            text.find(closing)
                .map(|position| position + closing.len_utf8())
        }) {
            text = &text[end..];
        } else {
            unquoted.push(character);
        }
    }
    unquoted
}

fn is_development_instruction(message: &str) -> bool {
    [
        "开发",
        "实现",
        "修复",
        "重构",
        "编写",
        "完成",
        "制作",
        "做一个",
        "做个",
        "develop",
        "implement",
        "build",
    ]
    .iter()
    .any(|word| message.contains(word))
}

pub struct DirectMessageParticipant<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub runtime: AgentRuntimeKind,
}

pub fn directed_message_runtimes(request: &DirectMessageRequest) -> Vec<AgentRuntimeKind> {
    let DirectMessageRequest::Directed { sender, recipient } = request else {
        return Vec::new();
    };
    let mut runtimes = Vec::new();
    for reference in [sender, recipient] {
        for runtime in [
            AgentRuntimeKind::ClaudeCode,
            AgentRuntimeKind::Codex,
            AgentRuntimeKind::KimiCode,
            AgentRuntimeKind::Pi,
            AgentRuntimeKind::OpenCode,
            AgentRuntimeKind::DeepSeekHarness,
            AgentRuntimeKind::ZCode,
            AgentRuntimeKind::ZCodeCli,
        ] {
            if runtime_aliases(runtime)
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(reference.trim_start_matches('@')))
                && !runtimes.contains(&runtime)
            {
                runtimes.push(runtime);
            }
        }
    }
    runtimes
}

/// Resolve only inside the explicitly selected roster. Exact instance names take precedence.
pub fn directed_sender_id<'a>(
    request: &DirectMessageRequest,
    participants: &[DirectMessageParticipant<'a>],
) -> Result<Option<(&'a str, &'a str)>, String> {
    let DirectMessageRequest::Directed { sender, recipient } = request else {
        return Ok(None);
    };
    let resolve = |reference: &str| {
        let reference = reference.trim_start_matches('@').trim();
        let named = participants
            .iter()
            .filter(|participant| {
                participant.id.eq_ignore_ascii_case(reference)
                    || participant.title.eq_ignore_ascii_case(reference)
                    || participant
                        .title
                        .split('·')
                        .skip(1)
                        .any(|name| name.trim().eq_ignore_ascii_case(reference))
            })
            .collect::<Vec<_>>();
        let mut matches = if named.is_empty() {
            participants
                .iter()
                .filter(|participant| {
                    runtime_aliases(participant.runtime)
                        .iter()
                        .any(|alias| alias.eq_ignore_ascii_case(reference))
                })
                .collect::<Vec<_>>()
        } else {
            named
        };
        // A selected CLI group may be called "zcode" in the instruction body.
        // Resolve this shorthand only within the selected roster; never add Desktop.
        if matches.is_empty() && reference.eq_ignore_ascii_case("zcode") {
            matches = participants.iter()
                .filter(|participant| participant.runtime == AgentRuntimeKind::ZCodeCli)
                .collect();
        }
        match matches.as_slice() {
            [participant] => Ok(participant.id),
            [] => Err(format!(
                "无法在已选择的 Agent 中找到“{reference}”，请明确指定发送者和接收者"
            )),
            _ => Err(format!(
                "“{reference}”对应多个已选择会话，请使用具体 Agent 名称或完整会话 ID"
            )),
        }
    };
    let sender = resolve(sender)?;
    let recipient = resolve(recipient)?;
    if sender == recipient {
        return Err("定向通信的发送者和接收者不能是同一个会话".into());
    }
    Ok(Some((sender, recipient)))
}

fn runtime_aliases(runtime: AgentRuntimeKind) -> &'static [&'static str] {
    match runtime {
        AgentRuntimeKind::ClaudeCode => &[
            "claude",
            "claude code",
            "claudecode",
            "claude_code",
            "claude-code",
        ],
        AgentRuntimeKind::Codex => &["codex", "codex cli"],
        AgentRuntimeKind::KimiCode => &["kimi", "kimi code", "kimicode", "kimi_code"],
        AgentRuntimeKind::MiniMaxCode | AgentRuntimeKind::Unsupported => &[],
        AgentRuntimeKind::Pi => &["pi", "pi agent"],
        AgentRuntimeKind::OpenCode => &["opencode", "open code"],
        AgentRuntimeKind::DeepSeekHarness => &["deepseek harness", "deepseek_harness", "dsh"],
        AgentRuntimeKind::ClaudeChat => &["claude chat", "claude_chat"],
        AgentRuntimeKind::KimiChat => &["kimi chat", "kimi_chat"],
        AgentRuntimeKind::PiChat => &["pi chat", "pi_chat"],
        AgentRuntimeKind::CodexChat => &["codex chat", "codex_chat"],
        AgentRuntimeKind::OpenCodeChat => &["opencode chat", "opencode_chat"],
        AgentRuntimeKind::ZCode => &["zcode"],
        AgentRuntimeKind::ZCodeCli => &["zcode cli", "zcode_cli", "zcodecli", "zcode-cli"],
        AgentRuntimeKind::OhMyPi | AgentRuntimeKind::Antigravity => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toward_codex_is_directed_and_a_misspelled_sender_is_not_broadcast() {
        for separator in ["给", "向", "对"] {
            assert_eq!(direct_message_request(&format!("@ClaudeCode @Codex claude code{separator}codex打个招呼")),
                Some(DirectMessageRequest::Directed { sender: "claude code".into(), recipient: "codex".into() }));
        }
        let request = direct_message_request("@ClaudeCode @Codex claude coide向codex打个招呼").unwrap();
        assert!(matches!(request, DirectMessageRequest::Directed { .. }));
        assert!(directed_sender_id(&request, &[]).is_err());
    }

    #[test]
    fn zcode_cli_directed_runtime_is_supported() {
        let request = DirectMessageRequest::Directed { sender: "zcode_cli".into(), recipient: "zcode".into() };
        assert_eq!(directed_message_runtimes(&request), vec![AgentRuntimeKind::ZCodeCli, AgentRuntimeKind::ZCode]);
    }

    #[test]
    fn original_directed_message_is_not_a_development_plan() {
        let request = direct_message_request(
            "@Pi审查工程师 @Claude架构师 让pi给claude code发一句 hello，我是pi！！！",
        )
        .unwrap();
        assert_eq!(
            request,
            DirectMessageRequest::Directed {
                sender: "pi".into(),
                recipient: "claude code".into()
            }
        );
        let roster = [
            DirectMessageParticipant {
                id: "pi-session",
                title: "Pi · Pi审查工程师",
                runtime: AgentRuntimeKind::Pi,
            },
            DirectMessageParticipant {
                id: "claude-session",
                title: "Claude Code · Claude架构师",
                runtime: AgentRuntimeKind::ClaudeCode,
            },
        ];
        assert_eq!(
            directed_sender_id(&request, &roster).unwrap(),
            Some(("pi-session", "claude-session"))
        );
    }

    #[test]
    fn short_message_phrases_use_directed_delivery() {
        let expected = Some(DirectMessageRequest::Directed {
            sender: "pi".into(),
            recipient: "claude code".into(),
        });
        for phrase in [
            "发句hi",
            "发一句 hi",
            "说句hi",
            "说一句 hi",
            "发条消息hi",
            "发一条消息 hi",
            "发个消息 hi",
        ] {
            let message = format!("@Pi审查工程师 @Claude架构师 pi给claude code{phrase}");
            assert_eq!(direct_message_request(&message), expected, "{message}");
        }
        assert_eq!(
            direct_message_request(
                "@Pi审查工程师 @Claude架构师 pi给claude code发句hi，然后一起开发游戏"
            ),
            None
        );
    }

    #[test]
    fn colloquial_group_chat_uses_direct_delivery_without_planning() {
        for message in [
            "@ClaudeCode @Codex @ZCodeCLI 你们三个聊下",
            "@模型验收-Claude @模型验收-Codex @聊天-ZCodeCLI 你三聊个天",
            "@ClaudeCode @Codex 聊一下",
            "@ClaudeCode @Codex 聊一聊",
            "@ClaudeCode @Codex 聊聊",
            "@ClaudeCode @Codex 你们三个聊下。本轮验收；收到 PING 时，通过原会话工具向包装中原发送方回复 ACK。",
        ] {
            assert_eq!(direct_message_request(message), Some(DirectMessageRequest::Broadcast), "{message}");
        }
        for message in [
            "@ClaudeCode @Codex 先聊下，然后开发游戏",
            "@ClaudeCode @Codex 修复聊个天功能",
            "@ClaudeCode @Codex 聊一下并实现接口",
            "@ClaudeCode @Codex 开发聊天应用",
        ] {
            assert_eq!(direct_message_request(message), None, "{message}");
        }
    }

    #[test]
    fn self_introduction_uses_direct_messages_but_implementation_stays_planned() {
        for message in [
            "@ClaudeCode @Codex 你两自我介绍一下",
            "@ClaudeCode @Codex 你们互相介绍一下自己",
            "@模型验收-Claude @模型验收-Codex 请各自介绍自己",
            "@ClaudeCode @Codex 相互介绍一下",
            "@ClaudeCode @Codex 彼此介绍一下",
            "@ClaudeCode @Codex 互相认识一下",
        ] {
            assert_eq!(direct_message_request(message), Some(DirectMessageRequest::Broadcast), "{message}");
        }
        for message in [
            "@ClaudeCode @Codex 开发一个自我介绍页面",
            "@ClaudeCode @Codex 先自我介绍，然后实现聊天应用",
            "@ClaudeCode @Codex 做一个自我介绍生成器",
            "@ClaudeCode @Codex 完成自我介绍功能的重构",
        ] {
            assert_eq!(direct_message_request(message), None, "{message}");
        }
        assert!(!self_introduction_requested("@自我介绍助手 @Codex 聊一下"));
        assert!(!self_introduction_requested("@ClaudeCode @Codex 聊一下“自我介绍”这几个字"));
    }

    #[test]
    fn greetings_stay_broadcast_and_development_stays_planned() {
        assert_eq!(
            direct_message_request("@OpenCode设计师 @Pi审查工程师 你两打个招呼"),
            Some(DirectMessageRequest::Broadcast)
        );
        for message in [
            "@claude @codex 一起开发俄罗斯方块，claude负责前端，codex负责后端",
            "@Pi审查工程师 修复发消息功能",
            "@Pi审查工程师 @Claude架构师 开发游戏并互相通信",
        ] {
            assert_eq!(direct_message_request(message), None, "{message}");
        }
        assert_eq!(direct_message_request("让pi给claude code发一句 hello，然后两人一起开发俄罗斯方块，pi负责后端claude负责前端"), None);
        assert!(matches!(
            direct_message_request("让pi给claude code发一句 请修复接口"),
            Some(DirectMessageRequest::Directed { .. })
        ));
        for body in [
            "「先修复接口，然后完成验证」",
            "“先修复接口，然后完成验证”",
            "\"先修复接口，然后完成验证\"",
            "'先修复接口，然后完成验证'",
        ] {
            assert!(matches!(
                direct_message_request(&format!("让pi给claude code发一句{body}")),
                Some(DirectMessageRequest::Directed { .. })
            ));
        }
        assert_eq!(
            direct_message_request("让pi给claude code发一句「hello」，然后两人一起开发游戏"),
            None
        );
    }

    #[test]
    fn all_active_runtimes_can_be_sender_or_recipient() {
        let runtimes = [
            AgentRuntimeKind::ClaudeCode,
            AgentRuntimeKind::Codex,
            AgentRuntimeKind::KimiCode,
            AgentRuntimeKind::Pi,
            AgentRuntimeKind::OpenCode,
        ];
        for sender in runtimes {
            for recipient in runtimes {
                if sender == recipient {
                    continue;
                }
                let roster = [
                    DirectMessageParticipant {
                        id: "sender",
                        title: "sender instance",
                        runtime: sender,
                    },
                    DirectMessageParticipant {
                        id: "recipient",
                        title: "recipient instance",
                        runtime: recipient,
                    },
                ];
                let request = direct_message_request(&format!(
                    "让{}给{}发一句 hello",
                    runtime_aliases(sender)[0],
                    runtime_aliases(recipient)[0]
                ))
                .unwrap();
                assert_eq!(
                    directed_sender_id(&request, &roster).unwrap(),
                    Some(("sender", "recipient"))
                );
            }
        }
    }

    #[test]
    fn ambiguous_or_unselected_endpoints_fail_without_broadcast() {
        let roster = [
            DirectMessageParticipant {
                id: "a",
                title: "Pi · 审查员",
                runtime: AgentRuntimeKind::Pi,
            },
            DirectMessageParticipant {
                id: "b",
                title: "Pi · 测试员",
                runtime: AgentRuntimeKind::Pi,
            },
            DirectMessageParticipant {
                id: "c",
                title: "Claude Code · 架构师",
                runtime: AgentRuntimeKind::ClaudeCode,
            },
        ];
        assert!(directed_sender_id(
            &direct_message_request("让pi给claude code发一句 hello").unwrap(),
            &roster
        )
        .unwrap_err()
        .contains("多个"));
        assert!(directed_sender_id(
            &direct_message_request("让codex给claude code发一句 hello").unwrap(),
            &roster
        )
        .unwrap_err()
        .contains("无法"));
        assert_eq!(
            directed_sender_id(
                &direct_message_request("让审查员给架构师说一句 hello").unwrap(),
                &roster
            )
            .unwrap(),
            Some(("a", "c"))
        );
        assert!(directed_sender_id(
            &direct_message_request("让审查员给审查员发一句 hello").unwrap(),
            &roster
        )
        .is_err());
    }
}
