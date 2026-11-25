import { LocalizedErrorText } from "./LocalizedErrorText";
import { useTranslation } from "../useInterfaceLanguage";
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { sharedMemoryCommands, type SharedMemoryCommandStatus } from "../lib/sharedMemoryCommands";

/** Only the optional project guard scripts consult this file; native HTTP clients do not. */
export function AdditionalProjectAccess({ projectDir, preview }: { projectDir: string; preview: boolean }) {
  const t = useTranslation();
  const [allowed, setAllowed] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState("");
  const [retry, setRetry] = useState(0);
  useEffect(() => {
    let current = true;
    setBusy(true);
    const read = preview ? Promise.resolve(false) : invoke<boolean>("ai_memory_project_access", { projectDir, grant: null });
    read.then(value => { if (current) { setAllowed(value); setError(""); } })
      .catch(e => { if (current) { setAllowed(null); setError(String(e)); } })
      .finally(() => { if (current) setBusy(false); });
    return () => { current = false; };
  }, [projectDir, preview, retry]);
  async function toggle() {
    if (busy || allowed === null) return;
    setBusy(true);
    try {
      const value = preview ? !allowed : await invoke<boolean>("ai_memory_project_access", { projectDir, grant: !allowed });
      setAllowed(value);
      setError("");
    } catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  }
  return <div className="sm-row-detail">
    <strong>{t("附加项目授权")}</strong>
    <p>{t("项目桥和受限钩子默认仅在脚本所在项目运行。选择带 .ai-memory.toml 的其他项目根目录，可允许脚本在此运行并使用该项目的默认坐标。服务端权限、显式跨项目调用及原生 HTTP 接入由各自配置控制。")}</p>
    <button type="button" className="sm-button" disabled={busy} onClick={allowed === null ? () => setRetry(n => n + 1) : toggle}>
      {busy ? t("正在读取或保存…") : allowed === null ? t("重新读取项目授权") : allowed ? t("撤销附加授权") : t("授权所选项目")}
    </button>
    {error && <p role="alert"><LocalizedErrorText source={error} /></p>}
  </div>;
}

interface Session { id: string; startedAt: string; cwd: string; }
export function ZcodeFinalize({ projectDir, status, preview, copy }: {
  projectDir: string | null; status: SharedMemoryCommandStatus | null; preview: boolean;
  copy: (command: string) => void;
}) {
  const t = useTranslation();
  const [sessions, setSessions] = useState<Session[] | null>(null);
  const [id, select] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function load() {
    if (!projectDir || busy) return;
    setBusy(true); select(""); setSessions(null); setError("");
    try {
      const result = preview ? [] : await invoke<Session[]>("ai_memory_zcode_sessions", { projectDir });
      setSessions(result);
    } catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  }
  const commands = sharedMemoryCommands(status, { projectDir, sessionId: id });
  return <section><h4>{t("ZCode 手动收尾")}</h4>
    <p>{t("选择验证项目，再按开始时间和记忆会话 ID 核对要结束的会话。")}</p>
    <button type="button" className="sm-button" disabled={!projectDir || !status || busy} onClick={load}>{busy ? t("查询中…") : t("查询未结束的 ZCode 会话")}</button>
    {error && <p role="alert"><LocalizedErrorText source={error} /></p>}
    {sessions?.length === 0 && <p>{t("此目录没有未结束的 ZCode 记忆会话。")}</p>}
    {!!sessions?.length && <label>{t("目标记忆会话")} <select value={id} onChange={e => select(e.target.value)}>
      <option value="">{t("选择要收尾的会话")}</option>
      {sessions.map(s => <option key={s.id} value={s.id}>{new Date(s.startedAt).toLocaleString()} · {s.id}</option>)}
    </select></label>}
    {commands?.finalize && <><p>{t("核对后在")} {commands.shellLabel} {t("中执行：")}</p><pre className="sm-command">{commands.finalize}</pre>
      <button type="button" className="sm-button" onClick={() => copy(commands.finalize!)}>{t("复制指定会话收尾命令")}</button></>}
  </section>;
}
