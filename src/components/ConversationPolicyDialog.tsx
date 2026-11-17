import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import type { AgentRuntimeKind, ConversationHardBoundary, ConversationMigrationPreview, ConversationPolicyEntry, ConversationSnapshot } from "../types";
import { RUNTIME_LABELS } from "../runtimeLogos";
import { Modal } from "./ui/Modal";
import "../styles/conversation-policy.css";

/** Separate from role editing: saving a boundary never rewrites the role or project goal. */
export function ConversationPolicyDialog({ projectId, agentId = null, name, runtimeKind, allowMigration = true, onMigrated, onClose }: {
  projectId: string;
  agentId?: string | null;
  name: string;
  runtimeKind?: AgentRuntimeKind;
  allowMigration?: boolean;
  onMigrated?: (text: string) => void;
  onClose: () => void;
}) {
  const [snapshot, setSnapshot] = useState<ConversationSnapshot | null>(null);
  const [boundary, setBoundary] = useState<ConversationHardBoundary | null>(null);
  const [replacement, setReplacement] = useState("");
  const [preview, setPreview] = useState<ConversationMigrationPreview | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [receipt, setReceipt] = useState("");
  const inFlight = useRef(false);
  const close = useRef<HTMLButtonElement>(null);
  const entryFor = (value: ConversationSnapshot): ConversationPolicyEntry | undefined => agentId
    ? value.memberPolicies?.find(entry => entry.agentId === agentId)
    : value.projectPolicy;
  const entry = snapshot ? entryFor(snapshot) : undefined;
  const capability = snapshot?.runtimeCapabilities?.find(value => value.runtimeKind === runtimeKind);
  useEffect(() => {
    let current = true;
    void invoke<ConversationSnapshot>("ma_conversation_snapshot", { projectId }).then(value => {
      if (!current) return;
      const policy = entryFor(value);
      if (!policy || policy.projectId !== projectId) throw Error("群聊权限信息不完整，请重新读取。");
      setSnapshot(value); setBoundary(structuredClone(policy.boundary)); setReplacement(policy.legacyText);
    }).catch(reason => { if (current) setError(`读取群聊限制失败：${String(reason)}`); });
    return () => { current = false; };
  }, [projectId, agentId]);
  const run = async (work: () => Promise<void>) => {
    if (inFlight.current) return;
    inFlight.current = true; setBusy(true); setError(""); setReceipt("");
    try { await work(); } catch (reason) { setError(String(reason)); }
    finally { inFlight.current = false; setBusy(false); }
  };
  return createPortal(<Modal className="collab-agent-editor conversation-policy-dialog" overlayClassName="dialog-overlay collab-agent-editor-overlay" ariaLabel="群聊工具与长期限制" initialFocusRef={close} onClose={() => { if (!inFlight.current) onClose(); }}>
    <header><h3>{name} · 群聊工具与长期限制</h3><button ref={close} type="button" aria-label="关闭群聊限制" disabled={busy} onClick={onClose}>×</button></header>
    <p>职责决定专长与分工。群聊可使用已接通的只读研究工具；修改项目仍走任务授权。以下限制仅用于团队群聊，项目与成员限制取交集。</p>
    {!boundary && !error && <p role="status">正在读取群聊限制…</p>}
    {boundary && <>
      {runtimeKind && <p className="conversation-policy-capability">{RUNTIME_LABELS[runtimeKind]}：{capability ? `${capability.status === "adapted" ? "已适配，启动时核验" : "暂不支持受控群聊"} · ${capability.detail}` : "尚未取得能力信息，不能据此判断工具可用。"}</p>}
      {agentId && snapshot?.projectPolicy && (snapshot.projectPolicy.boundary.tools === "none" || snapshot.projectPolicy.boundary.network === "none" || snapshot.projectPolicy.boundary.deniedReadPaths.length > 0) && <p>项目还设置了：{[snapshot.projectPolicy.boundary.tools === "none" && "禁用工具", snapshot.projectPolicy.boundary.network === "none" && "禁止联网", snapshot.projectPolicy.boundary.deniedReadPaths.length > 0 && `禁读路径 ${snapshot.projectPolicy.boundary.deniedReadPaths.join("、")}`].filter(Boolean).join("；")}。成员设置不能放宽项目限制。</p>}
      <label>研究工具<select value={boundary.tools} disabled={busy || !!preview} onChange={event => setBoundary({ ...boundary, tools: event.target.value as ConversationHardBoundary["tools"] })}><option value="research">允许受控的读取与研究</option><option value="none">只回复，禁用工具</option></select></label>
      <label>公网查询<select value={boundary.network} disabled={busy || !!preview} onChange={event => setBoundary({ ...boundary, network: event.target.value as ConversationHardBoundary["network"] })}><option value="public_web">允许公开网页查询</option><option value="none">禁止联网</option></select></label>
      <label>禁止读取的路径<textarea rows={3} disabled={busy || !!preview} value={boundary.deniedReadPaths.join("\n")} onChange={event => setBoundary({ ...boundary, deniedReadPaths: event.target.value.split("\n") })} placeholder="每行一个路径，可填写项目内相对路径或绝对路径" /></label>
      <small>禁用工具时，公网查询同样不可用。运行时若无法落实限制，会拒绝启动并说明原因；“已适配”不代表已通过本机真实调用验证。</small>
      {!preview && <div><button type="button" className="btn secondary" disabled={busy} onClick={() => void run(async () => {
        const value = await invoke<ConversationSnapshot>("ma_conversation_policy_set", { input: { projectId, agentId, boundary: { ...boundary, deniedReadPaths: boundary.deniedReadPaths.map(value => value.trim()).filter(Boolean) } } });
        const saved = entryFor(value); if (!saved) throw Error("保存回执缺少群聊限制，请重新读取核对。");
        setSnapshot(value); setBoundary(structuredClone(saved.boundary)); setReceipt("群聊限制已保存，职责与项目目标未改动。");
      })}>保存群聊限制</button></div>}
      {entry?.legacyReviewRequired && <section className="conversation-policy-migration" aria-label="旧验收规则迁移">
        <h4>旧文字仍包含能力限制</h4><p>原文目前仍生效。可将长期职责改为专长描述，把原文完整保留为仅在选用时生效的验收规则。保存上方限制不会自动迁移。</p>
        {!allowMigration && <p role="status">请先保存或取消成员职责的草稿，再迁移旧规则。</p>}
        {!preview ? <><label>新的{agentId ? "职责定位" : "项目目标"}<textarea rows={4} value={replacement} disabled={busy || !allowMigration} onChange={event => setReplacement(event.target.value)} /></label><button className="btn secondary" disabled={busy || !allowMigration || replacement === entry.legacyText} onClick={() => void run(async () => {
          setPreview(await invoke<ConversationMigrationPreview>("ma_conversation_migration_preview", { input: { projectId, agentId, replacementText: replacement } }));
        })}>预览迁移</button></> : <>
          <label>修改前<textarea aria-label="修改前" readOnly rows={4} value={preview.previousText} /></label>
          <label>修改后<textarea aria-label="修改后" readOnly rows={4} value={preview.replacementText} /></label>
          <label>保留的验收规则<textarea aria-label="保留的验收规则" readOnly rows={4} value={preview.archivedRules} /></label>
          <small>确认后保存完整原文；后续可在群聊输入框的“本次规则”中选用。新建或清空会话不会删除规则。</small>
          <div className="conversation-policy-buttons"><button type="button" className="btn secondary" disabled={busy} onClick={() => setPreview(null)}>返回修改</button><button type="button" className="btn primary" disabled={busy} onClick={() => void run(async () => {
            const value = await invoke<ConversationSnapshot>("ma_conversation_migration_apply", { input: { projectId, previewId: preview.id } });
            const updated = entryFor(value); if (!updated) throw Error("迁移回执缺少最新规则，请重新读取核对。");
            setSnapshot(value); onMigrated?.(updated.legacyText); setPreview(null); setReceipt("迁移完成，原文已保留为验收规则。");
          })}>确认迁移并保留原规则</button></div>
        </>}
      </section>}
    </>}
    {receipt && <p role="status">{receipt}</p>}
    {error && <p className="collab-agent-error" role="alert">{error}</p>}
    <footer><button type="button" className="btn secondary" disabled={busy} onClick={onClose}>关闭</button></footer>
  </Modal>, document.body);
}
