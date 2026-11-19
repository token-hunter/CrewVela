import fs from 'node:fs';
import path from 'node:path';
import { randomUUID } from 'node:crypto';

// Pi 1.x exposes native MCP registration. Keep run-scoped credentials in the
// child environment instead of writing them into project/global configuration.
export function registerCollaborationMcp(pi, environment = process.env) {
  const endpoint = environment.CREWVELA_PI_MCP_URL;
  const token = environment.CREWVELA_AGENT_TOKEN;
  if (!endpoint || !token) return false;
  let url;
  try { url = new URL(endpoint); }
  catch { throw new Error('CrewVela Pi collaboration requires its managed local MCP endpoint'); }
  if (url.protocol !== 'http:' || url.hostname !== '127.0.0.1' || url.pathname !== '/mcp'
    || url.username || url.password || url.search || url.hash || url.port === '0') {
    throw new Error('CrewVela Pi collaboration requires its managed local MCP endpoint');
  }
  if (typeof pi.registerMcpServer !== 'function') {
    console.error('[CrewVela] This Pi version has no native MCP API; session CLI messaging remains available.');
    return false;
  }
  pi.registerMcpServer('crewvela', {
    url: endpoint,
    headers: { Authorization: 'Bearer ${CREWVELA_AGENT_TOKEN}' },
    exposure: 'direct',
    description: 'CrewVela project-scoped collaboration, messages and shared knowledge.',
  });
  return true;
}

// Private managed extension. Delivery calls Pi's API, never the focused TUI editor.
export default function crewvelaReadiness(pi) {
  registerCollaborationMcp(pi);
  const directory = process.env.CREWVELA_PI_EVENTS_DIR;
  const inbox = process.env.CREWVELA_PI_INBOX_DIR;
  const sessionId = process.env.CREWVELA_SESSION_ID;
  const token = process.env.CREWVELA_PI_TOKEN;
  if (!directory || !inbox || !sessionId || !token) return;
  const timerKey = Symbol.for('crewvela.pi-inbox-timer');
  const clockKey = Symbol.for('crewvela.pi-event-clock');
  if (globalThis[timerKey]) clearInterval(globalThis[timerKey]);
  let ctx, settled = false, agentWorking = false, prompts = 0, sequence = 0, inflight, lastReportedReady;
  const atomicWrite = (target, value) => {
    const temporary = `${target}.${randomUUID()}.tmp`;
    try {
      fs.mkdirSync(path.dirname(target), { recursive: true });
      fs.writeFileSync(temporary, JSON.stringify(value));
      fs.renameSync(temporary, target);
    } catch (error) {
      try { fs.unlinkSync(temporary); } catch {}
      throw error;
    }
  };
  const canReceive = () => Boolean(ctx && settled && prompts === 0 && ctx.isIdle() && !ctx.hasPendingMessages());
  const report = (allowReady, error, completion) => {
    // IEEE doubles lose nanosecond increments at epoch scale. A full millisecond
    // step keeps file/event order strict even when several hooks share Date.now().
    const now = Math.max(Date.now() * 1_000_000, (globalThis[clockKey] ?? 0) + 1_000_000);
    globalThis[clockKey] = now;
    try {
      const ready = Boolean(allowReady && canReceive() && !inflight);
      atomicWrite(path.join(directory, `${String(now).padStart(20, '0')}-${randomUUID()}.json`), {
        session_id: sessionId, _crewvela_app_session_id: sessionId,
        hook_event_name: 'RuntimePromptState', runtime_kind: 'pi', runtime_token: token,
        runtime_sequence: (sequence = Math.max(sequence + 1, Number(process.hrtime.bigint() / 1000n))),
        runtime_session_locator: ctx?.sessionManager?.getSessionFile?.(),
        ready,
        agent_working: agentWorking && prompts === 0,
        message_error: error,
        turn_completion: completion,
        _crewvela_queued_at: now,
      });
      lastReportedReady = ready;
    } catch (error) { console.error(`[CrewVela] Pi readiness event failed: ${error.message}`); }
  };
  const messageText = message => typeof message?.content === 'string' ? message.content :
    (message?.content ?? []).filter(item => item.type === 'text').map(item => item.text).join('\n');
  const poll = () => {
    if (!ctx) return;
    try {
      // session_start can precede idle initialization; failed input preflight can
      // also return to idle without a settlement hook. Reconcile actual API state,
      // keeping the settlement/modal/inflight gates and emitting only changes.
      if (!inflight && lastReportedReady !== canReceive()) report(true);
      if (inflight) {
        if (fs.existsSync(path.join(inbox, `${inflight.id}.retired.json`))) {
          inflight = undefined;
          report(true);
          return;
        }
        // Pi emits user events BEFORE appending them. Confirm on a subsequent loop from its session entries (in-memory receipt, not disk durability).
        const accepted = ctx.sessionManager.getEntries().some(entry => entry.type === 'message' &&
          entry.message?.role === 'user' && messageText(entry.message).includes(inflight.marker));
        if (accepted) {
          atomicWrite(path.join(inbox, `${inflight.id}.accepted.json`), { sessionId, messageId: inflight.id, status: 'accepted' });
          inflight = undefined;
          report(true);
        } else if (!inflight.warned && Date.now() - inflight.started >= 30_000) {
          inflight.warned = true;
          const error = `Pi消息 ${inflight.id} 未确认进入会话，未自动重发；请检查Pi错误和原收件队列`;
          atomicWrite(path.join(inbox, `${inflight.id}.unconfirmed.json`), { sessionId, messageId: inflight.id, error });
          report(false, error);
          ctx.ui?.notify?.(error, 'warning');
        }
        return;
      }
      if (!canReceive()) return;
      const requests = fs.readdirSync(inbox).filter(name => name.endsWith('.request.json')).sort();
      for (const name of requests) {
        const source = path.join(inbox, name);
        if (fs.existsSync(path.join(inbox, name.replace('.request.json', '.retired.json')))) continue;
        const request = JSON.parse(fs.readFileSync(source, 'utf8'));
        if (request.sessionId !== sessionId || request.token !== token || typeof request.body !== 'string') continue;
        const processing = path.join(inbox, `${request.messageId}.processing.json`);
        if (fs.existsSync(processing)) continue;
        fs.renameSync(source, processing); // claim before invoking a void asynchronous API; never blindly replay
        const marker = `[CrewVela message ${request.messageId}]`;
        inflight = { id: request.messageId, marker, started: Date.now(), warned: false };
        report(false);
        pi.sendUserMessage(`${marker}\n${request.body}`, { deliverAs: 'followUp', expandPromptTemplates: false });
        break;
      }
    } catch (error) {
      const description = `Pi程序化消息投递失败，未自动重发：${error.message}`;
      report(false, description);
      console.error(`[CrewVela] ${description}`);
    }
  };
  pi.on('session_start', (_event, context) => {
    ctx = context; settled = true; agentWorking = false; prompts = 0;
    fs.mkdirSync(inbox, { recursive: true });
    // Recover claimed deliveries by inspecting the current session, never by invoking them again.
    const unresolved = fs.readdirSync(inbox).filter(name => name.endsWith('.processing.json'))
      .find(name => !fs.existsSync(path.join(inbox, name.replace('.processing.json', '.accepted.json')))
        && !fs.existsSync(path.join(inbox, name.replace('.processing.json', '.retired.json'))));
    if (unresolved) {
      const previous = JSON.parse(fs.readFileSync(path.join(inbox, unresolved), 'utf8'));
      inflight = { id: previous.messageId, marker: `[CrewVela message ${previous.messageId}]`, started: Date.now() - 30_000, warned: false };
    }
    report(true);
  });
  // Input can fail model/auth preflight without any agent run or settlement event.
  // Only actual agent_start changes the settlement latch; the official idle/pending API gates delivery.
  pi.on('input', (_event, context) => { ctx = context; report(false); return { action: 'continue' }; });
  pi.on('agent_start', (_event, context) => { ctx = context; settled = false; agentWorking = true; report(false); });
  // agent_end may precede retries/compaction/continuation; only settlement is final.
  pi.on('agent_settled', (_event, context) => {
    ctx = context; settled = true; agentWorking = false;
    // Pair actual input with assistant output only after retries and continuations settle.
    let completion;
    if (ctx.isIdle() && !ctx.hasPendingMessages()) {
      const entries = ctx.sessionManager.getBranch();
      const input = entries.findLastIndex(entry => entry.type === 'message' && entry.message?.role === 'user');
      if (input >= 0) {
        const prompt = messageText(entries[input].message).replace(/^\[CrewVela message [^\]\r\n]+\]\n/, '');
        const assistant = entries.slice(input + 1).filter(entry => entry.type === 'message' && entry.message?.role === 'assistant');
        if (assistant.length) {
          const last = assistant.at(-1).message;
          const managed = process.env.CREWVELA_PI_MANAGED === '1';
          const summary = managed ? messageText(last) : assistant.map(entry => messageText(entry.message)).filter(Boolean).join('\n');
          const failed = managed ? last.stopReason !== 'stop' : ['error', 'aborted'].includes(last.stopReason);
          const error = failed ? (last.errorMessage || `Pi turn ${last.stopReason}`) : undefined;
          completion = { prompt, summary: error ? '' : summary, error, model: ctx.model ? `${ctx.model.provider}/${ctx.model.id}` : undefined };
        }
      }
    }
    report(true, undefined, completion);
  });
  pi.on('ui_prompt_start', (_event, context) => { ctx = context; prompts++; report(false); });
  pi.on('ui_prompt_end', (_event, context) => { ctx = context; prompts = Math.max(0, prompts - 1); report(true); });
  pi.on('session_shutdown', (_event, context) => {
    ctx = context; settled = false; agentWorking = false; report(false);
    clearInterval(globalThis[timerKey]); delete globalThis[timerKey];
  });
  globalThis[timerKey] = setInterval(poll, 100);
  globalThis[timerKey].unref?.();
}
