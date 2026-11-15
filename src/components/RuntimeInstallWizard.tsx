import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ArrowLeft, CheckCircle, Circle, WarningCircle, CaretDown, ArrowClockwise } from "@phosphor-icons/react";
import type { AgentRuntimeKind } from "../types";
import { RUNTIME_LABELS, RUNTIME_LOGOS } from "../runtimeLogos";
import { downloadPercent, installationRunning, installationStep, type RuntimeEnvironment, type RuntimeInstallJob } from "../runtimeInstallation";
import "../styles/runtime-installation.css";

export function RuntimeInstallWizard({ kind, onBack }: {
  kind: AgentRuntimeKind; onBack: () => void;
}) {
  const [environment, setEnvironment] = useState<RuntimeEnvironment | null>(null);
  const [job, setJob] = useState<RuntimeInstallJob | null>(null);
  const [loading, setLoading] = useState(true);
  const [starting, setStarting] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [statusError, setStatusError] = useState<string | null>(null);
  const [loginOpened, setLoginOpened] = useState(false);
  const [loginBusy, setLoginBusy] = useState(false);
  const alive = useRef(true);
  const generation = useRef(0);
  const completed = useRef<string | null>(null);
  const requestPending = useRef(false);
  const operationRevision = useRef(0);
  const label = RUNTIME_LABELS[kind];
  const running = installationRunning(job);
  const busy = running || starting;
  const step = installationStep(job);
  const percent = downloadPercent(job);
  const failed = job?.phase === "failed";
  const installed = job?.phase === "ready" || (!!environment?.version && !environment.error);
  const refresh = useCallback(async () => {
    const current = ++generation.current;
    setLoading(true);
    try {
      const result = await invoke<RuntimeEnvironment>("runtime_install_environment", { kind });
      if (alive.current && generation.current === current) { setEnvironment(result); setError(null); }
    } catch (reason) { if (alive.current && generation.current === current) setError(String(reason)); }
    finally { if (alive.current && generation.current === current) setLoading(false); }
  }, [kind]);
  useEffect(() => {
    alive.current = true;
    void refresh();
    let polling = false;
    const poll = async () => {
      if (polling) return;
      polling = true;
      const revision = operationRevision.current;
      try {
        const next = await invoke<RuntimeInstallJob | null>("runtime_install_status", { kind });
        if (!alive.current || revision !== operationRevision.current || requestPending.current) return;
        setStatusError(null);
        setJob(next);
        if (next && !installationRunning(next)) {
          setCancelling(false);
          if (completed.current !== next.id) { completed.current = next.id; void refresh(); }
        }
      } catch (reason) { if (alive.current) setStatusError(`读取安装状态失败：${String(reason)}`); }
      finally { polling = false; }
    };
    void poll();
    const timer = window.setInterval(() => void poll(), 600);
    return () => { alive.current = false; generation.current += 1; window.clearInterval(timer); };
  }, [kind, refresh]);
  const start = async () => {
    if (requestPending.current || running) return;
    operationRevision.current += 1;
    requestPending.current = true; setStarting(true); setError(null); setLoginOpened(false);
    try {
      const result = await invoke<RuntimeInstallJob>("runtime_install_start", { kind });
      if (alive.current) setJob(result);
    } catch (reason) { if (alive.current) setError(String(reason)); }
    finally { requestPending.current = false; if (alive.current) setStarting(false); }
  };
  const cancel = async () => {
    if (!job || cancelling) return;
    setCancelling(true); setError(null);
    try { await invoke("runtime_install_cancel", { id: job.id }); }
    catch (reason) { if (alive.current) { setError(String(reason)); setCancelling(false); } }
  };
  const login = async () => {
    if (!installed || busy || loginBusy) return;
    setLoginBusy(true); setError(null);
    try { await invoke("runtime_open_login", { kind }); if (alive.current) setLoginOpened(true); }
    catch (reason) { if (alive.current) setError(String(reason)); }
    finally { if (alive.current) setLoginBusy(false); }
  };
  const heading = failed ? "安装未完成" : job?.phase === "cancelled" ? "安装已取消" : running
    ? job?.phase === "verifying" ? "正在验证安装" : job?.phase === "checking" ? "正在检查依赖" : `正在安装 ${label}`
    : job?.phase === "ready" ? `${label} 安装已验证` : `准备${environment?.executable ? "升级或修复" : "安装"} ${label}`;
  const phaseMessage = job?.message ?? (loading ? "正在检测本机 CLI 与官方版本…" : "验证通过后才切换入口，原安装保留。现有会话可继续运行。");
  return <div className="runtime-installer" aria-busy={busy}>
    <button type="button" className="runtime-install-back" disabled={busy} onClick={onBack}><ArrowLeft size={19} />返回 Runtime 列表</button>
    <div className="runtime-install-identity"><img src={RUNTIME_LOGOS[kind]} alt="" /><div><h2>{environment?.executable ? "管理" : "安装"} {label}</h2><p>使用本机 CLI 配置，保留现有账号与模型</p></div></div>
    <ol className="runtime-install-steps" aria-label="安装步骤">{["检查依赖", "安装", "验证", "登录"].map((title, index) => {
      const done = index < step || (index === 3 && environment?.auth === "authenticated" && !busy && installed);
      return <li key={title} className={done ? "done" : index === step ? "current" : ""} aria-current={index === step ? "step" : undefined}>
        {done ? <CheckCircle size={27} weight="fill" /> : <Circle size={27} weight={index === step ? "duotone" : "regular"} />}<span>{title}</span>
      </li>;
    })}</ol>
    <p className="runtime-install-caption">安装供 CrewVela 使用，外部终端保留原 CLI；账号额度需另行确认。</p>
    <section className="runtime-install-progress" aria-label="安装进度">
      <h3>{heading}</h3>
      {busy && <div className="runtime-install-progress-row"><progress aria-label="下载进度" max={100} value={percent} />{percent !== undefined && <strong>{percent}%</strong>}</div>}
      <p role="status">{phaseMessage}</p>
      {(error || statusError) && <p role="alert" className="runtime-install-error"><WarningCircle size={18} />{error || statusError}</p>}
      {failed && <p role="alert" className="runtime-install-error"><WarningCircle size={18} />{job.error}</p>}
      {environment?.updateError && !busy && <p className="runtime-install-note">官方版本暂未确认：{environment.updateError}</p>}
    </section>
    <dl className="runtime-install-facts">
      <div><dt>依赖检查</dt><dd>{loading ? "正在检查…" : !environment ? "检查未完成，请重新检查" : environment.supported ? <><CheckCircle size={18} />当前平台支持，不需要 Node/npm</> : "此平台请使用官方安装方式"}</dd></div>
      <div><dt>安装方式</dt><dd>官方{kind === "codex" ? "原生平台包" : "原生程序"} · 当前用户目录</dd></div>
      <div><dt>原有配置</dt><dd>沿用本地配置</dd></div>
      {!busy && <><div><dt>版本</dt><dd>{loading ? "正在确认…" : environment?.version ?? "尚未确认"}{(job?.version ?? environment?.latestVersion) && <span className="runtime-install-latest">官方版本 {job?.version ?? environment?.latestVersion}</span>}</dd></div>
      <div><dt>登录状态</dt><dd>{environment?.auth === "authenticated" ? "已登录（未验证额度）" : environment?.auth === "login_required" ? "待登录" : "未验证，请在原生 CLI 中确认"}</dd></div></>}
    </dl>
    <details className="runtime-install-details"><summary><CaretDown size={17} />安装详情</summary><dl>
      <div><dt>当前来源</dt><dd>{environment?.source ?? "正在检测…"}</dd></div>
      <div><dt>当前路径</dt><dd>{environment?.executable ?? "未检测到可用 CLI"}</dd></div>
      <div><dt>安装目录</dt><dd>{environment?.installDirectory ?? "正在检测…"}</dd></div>
      {environment?.error && <div><dt>当前安装检查</dt><dd>{environment.error}</dd></div>}
      <div><dt>文件校验</dt><dd>{kind === "codex" ? "官方 npm 平台包 SHA512" : "官方发行清单 SHA256"}</dd></div>
      {busy && <div><dt>目标版本</dt><dd>{job?.version ?? "正在确认…"}</dd></div>}
    </dl></details>
    {loginOpened && <p className="runtime-install-note" role="status">已打开原生 CLI 登录窗口。完成后点击“重新检查”，登录信息由 CLI 自行管理。</p>}
    <footer className="runtime-install-footer">
      {busy ? <button className="btn secondary" disabled={starting || cancelling} onClick={() => void cancel()}>{cancelling ? "正在取消…" : "取消安装"}</button>
        : <button className="btn secondary" disabled={loading || !environment?.supported} onClick={() => void start()}>{failed || job?.phase === "cancelled" ? "重试安装" : installed ? "升级或修复安装" : "开始安装"}</button>}
      <span>{busy ? "关闭面板后继续，可返回查看" : installed ? "安装与登录状态分别确认" : "验证通过后可登录"}</span>
      {!busy && <button className="btn secondary" disabled={loading} onClick={() => void refresh()}><ArrowClockwise size={16} />重新检查</button>}
      <button className="btn runtime-install-primary" disabled={!installed || busy || loginBusy} onClick={() => void login()}>{loginBusy ? "正在打开…" : "打开 CLI 登录"}</button>
    </footer>
  </div>;
}
