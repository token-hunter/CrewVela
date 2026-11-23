import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ArrowLeft, CheckCircle, Circle, WarningCircle, CaretDown, ArrowClockwise } from "@phosphor-icons/react";
import type { AgentRuntimeKind } from "../types";
import { RUNTIME_LABELS, RUNTIME_LOGOS } from "../runtimeLogos";
import { downloadPercent, installationRunning, installationStep, runtimeInstallationInfo, supportsChatInstallation, type RuntimeEnvironment, type RuntimeInstallJob } from "../runtimeInstallation";
import "../styles/runtime-installation.css";

export function RuntimeInstallWizard({ kind, onBack, onComplete = onBack }: {
  kind: AgentRuntimeKind; onBack: () => void; onComplete?: () => void;
}) {
  const [environment, setEnvironment] = useState<RuntimeEnvironment | null>(null);
  const [job, setJob] = useState<RuntimeInstallJob | null>(null);
  const [loading, setLoading] = useState(true);
  const [latestVersion, setLatestVersion] = useState<string | null>(null);
  const [latestLoading, setLatestLoading] = useState(true);
  const [latestError, setLatestError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [statusError, setStatusError] = useState<string | null>(null);
  const [loginOpened, setLoginOpened] = useState(false);
  const [loginBusy, setLoginBusy] = useState(false);
  const [selectingCli, setSelectingCli] = useState(false);
  const alive = useRef(true);
  const generation = useRef(0);
  const completed = useRef<string | null>(null);
  const requestPending = useRef(false);
  const operationRevision = useRef(0);
  const environmentRequest = useRef<Promise<void> | null>(null);
  const latestRequest = useRef<Promise<void> | null>(null);
  const lastPolledJob = useRef<RuntimeInstallJob | null>(null);
  const label = RUNTIME_LABELS[kind];
  const isChat = supportsChatInstallation(kind);
  const isLocalCli = kind === "zcode_cli";
  const chat = environment?.chat;
  const info = runtimeInstallationInfo(kind);
  const running = installationRunning(job);
  const busy = running || starting;
  const currentJob = isChat && job?.phase === "ready" && !chat?.verified ? null : job;
  const step = isChat && chat?.verified && !busy ? 3 : installationStep(currentJob);
  const percent = downloadPercent(job);
  const failed = job?.phase === "failed";
  const installed = isChat ? !!chat?.verified : job?.phase === "ready" || (!!environment?.version && !environment.error);
  const refresh = useCallback(() => {
    if (environmentRequest.current) return environmentRequest.current;
    const current = ++generation.current;
    setLoading(true);
    const pending = invoke<RuntimeEnvironment>("runtime_install_environment", { kind })
      .then(result => { if (alive.current && generation.current === current) { setEnvironment(result); setError(null); } })
      .catch(reason => { if (alive.current && generation.current === current) {
        if (isChat) setEnvironment(null);
        setError(String(reason));
      } })
      .finally(() => {
        if (environmentRequest.current === pending) environmentRequest.current = null;
        if (alive.current && generation.current === current) setLoading(false);
      });
    environmentRequest.current = pending;
    return pending;
  }, [kind, isChat]);
  const refreshLatest = useCallback(() => {
    if (isChat || isLocalCli) return;
    if (latestRequest.current) return latestRequest.current;
    setLatestLoading(true);
    const pending = invoke<string>("runtime_install_latest_version", { kind })
      .then(version => { if (alive.current) { setLatestVersion(version); setLatestError(null); } })
      .catch(reason => { if (alive.current) setLatestError(String(reason)); })
      .finally(() => { if (latestRequest.current === pending) latestRequest.current = null; if (alive.current) setLatestLoading(false); });
    latestRequest.current = pending;
    return pending;
  }, [kind, isChat, isLocalCli]);
  useEffect(() => {
    alive.current = true;
    let firstFrame = 0, backgroundFrame = 0, timer = 0;
    let polling = false;
    const poll = async () => {
      if (polling) return;
      polling = true;
      const revision = operationRevision.current;
      try {
        const next = await invoke<RuntimeInstallJob | null>("runtime_install_status", { kind });
        if (!alive.current || revision !== operationRevision.current || requestPending.current) return;
        setStatusError(null);
        const wasRunning = installationRunning(lastPolledJob.current);
        lastPolledJob.current = next;
        setJob(next);
        if (next && !installationRunning(next)) {
          setCancelling(false);
          if (completed.current !== next.id) {
            completed.current = next.id;
            if (wasRunning) { generation.current += 1; environmentRequest.current = null; }
            void refresh();
            void refreshLatest();
          }
        }
      } catch (reason) { if (alive.current) setStatusError(`读取安装状态失败：${String(reason)}`); }
      finally { polling = false; }
    };
    // Give the dialog a painted frame before starting CLI processes or IPC.
    firstFrame = requestAnimationFrame(() => {
      backgroundFrame = requestAnimationFrame(() => {
        if (isLocalCli) { void refresh(); return; }
        void refreshLatest();
        void poll().finally(() => { if (alive.current && !installationRunning(lastPolledJob.current)) void refresh(); });
        timer = window.setInterval(() => void poll(), 600);
      });
    });
    return () => { alive.current = false; generation.current += 1; environmentRequest.current = null; latestRequest.current = null; window.cancelAnimationFrame(firstFrame); window.cancelAnimationFrame(backgroundFrame); window.clearInterval(timer); };
  }, [kind, isLocalCli, refresh, refreshLatest]);
  const changeLocalCli = async (reset = false) => {
    if (requestPending.current || loading || !environment?.localCli?.canSelect) return;
    requestPending.current = true;
    setSelectingCli(true);
    setError(null);
    try {
      const path = reset ? null : await open({ title: "选择已有 ZCode CLI", multiple: false, directory: false,
        filters: [{ name: "ZCode CLI", extensions: ["exe", "cmd", "bat", "cjs", "js"] }] });
      if (!alive.current || (!reset && typeof path !== "string")) return;
      const result = await invoke<RuntimeEnvironment>("runtime_set_zcode_cli_path", { path });
      if (alive.current) setEnvironment(result);
    } catch (reason) { if (alive.current) setError(String(reason)); }
    finally { requestPending.current = false; if (alive.current) setSelectingCli(false); }
  };
  const start = async () => {
    if (requestPending.current || running) return;
    operationRevision.current += 1;
    requestPending.current = true; setStarting(true); setError(null); setLoginOpened(false);
    try {
      const result = await invoke<RuntimeInstallJob>("runtime_install_start", { kind });
      if (alive.current) {
        lastPolledJob.current = result;
        setJob(result);
      }
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
    : isChat ? installed ? `${label} 启动验证通过` : `准备使用 ${label}`
      : job?.phase === "ready" ? `${label} 安装已验证` : `准备${environment?.executable ? "升级或修复" : "安装"} ${label}`;
  const phaseMessage = currentJob?.message ?? (isChat
    ? loading ? "正在检查本机启动依赖…" : installed ? "启动协议验证通过。登录和模型可用性需单独确认。" : null
    : loading ? "正在检测本机 CLI 与官方版本…" : "验证通过后才切换入口，原安装保留。现有会话可继续运行。");
  const authLabel = environment?.auth === "authenticated" ? "已登录（未验证额度）" : environment?.auth === "login_required" ? "待登录" : "请在原生运行程序中确认";
  const officialUrl = chat?.officialUrl;
  const installAction = failed || job?.phase === "cancelled" ? "重试安装" : !installed ? "开始安装"
    : kind !== "antigravity" || latestVersion == null ? "升级或修复安装"
    : latestVersion === environment?.version ? "修复安装" : "更新";
  let chatAction;
  if (chat?.canInstall && !installed) {
    chatAction = <button type="button" className="btn runtime-install-primary" disabled={loading} onClick={() => void start()}>{failed || job?.phase === "cancelled" ? "重试安装" : "安装并验证"}</button>;
  } else if (installed && chat?.canLogin && environment?.auth !== "authenticated") {
    chatAction = <button type="button" className="btn runtime-install-primary" disabled={loginBusy || loading} onClick={() => void login()}>{loginBusy ? "正在打开…" : info.loginLabel}</button>;
  } else if (!installed && officialUrl) {
    chatAction = <button type="button" className="btn runtime-install-primary" onClick={() => void openUrl(officialUrl).catch(reason => setError(String(reason)))}>打开官方安装页</button>;
  } else if (installed) {
    chatAction = <button type="button" className="btn runtime-install-primary" onClick={onComplete}>完成</button>;
  }
  if (isLocalCli) return <div className="runtime-installer" aria-busy={loading || selectingCli}>
    <button type="button" className="runtime-install-back" onClick={onBack}><ArrowLeft size={19} />返回 Runtime 列表</button>
    <div className="runtime-install-identity"><img src={RUNTIME_LOGOS[kind]} alt="" /><div><h2>管理 {label}</h2><p>使用本机 ZCode CLI</p></div></div>
    <dl className="runtime-install-facts">
      <div><dt>当前版本</dt><dd>{loading ? "正在检查…" : environment?.version ?? "尚未确认"}</dd></div>
      <div><dt>当前来源</dt><dd>{environment?.source ?? "尚未确认"}</dd></div>
      <div><dt>启动路径</dt><dd>{environment?.executable ?? "未检测到可用 CLI"}</dd></div>
      <div><dt>诊断结果</dt><dd>{loading ? "正在检查…" : environment?.localCli?.diagnosis ?? environment?.error ?? "尚未完成"}</dd></div>
    </dl>
    {error && <p role="alert" className="runtime-install-error"><WarningCircle size={18} />{error}</p>}
    {environment?.localCli?.canSelect === false && <p className="runtime-install-note">当前路径由 CREWVELA_ZCODE_CLI_EXECUTABLE 指定。</p>}
    <footer className="runtime-install-footer">
      <button type="button" className="btn secondary" disabled={loading || selectingCli} onClick={() => void refresh()}><ArrowClockwise size={16} />重新检查</button>
      {environment?.localCli?.canSelect && environment.localCli.hasSelection && <button type="button" className="btn secondary" disabled={loading || selectingCli} onClick={() => void changeLocalCli(true)}>恢复自动检测</button>}
      {environment?.localCli?.canSelect && <button type="button" className="btn runtime-install-primary" disabled={loading || selectingCli} onClick={() => void changeLocalCli()}>选择已有 CLI</button>}
    </footer>
  </div>;
  return <div className="runtime-installer" aria-busy={busy}>
    <button type="button" className="runtime-install-back" disabled={busy} onClick={onBack}><ArrowLeft size={19} />返回 Runtime 列表</button>
    <div className="runtime-install-identity"><img src={RUNTIME_LOGOS[kind]} data-runtime={kind} alt="" /><div><h2>{isChat || environment?.executable ? "管理" : "安装"} {label}</h2><p>{isChat ? "沿用本机配置、账号和模型" : "使用本机 CLI 配置，保留现有账号与模型"}</p></div></div>
    <ol className="runtime-install-steps" aria-label="安装步骤">{["检查依赖", "安装", "验证", "登录"].map((title, index) => {
      const done = index < step || (index === 3 && environment?.auth === "authenticated" && !busy && installed);
      return <li key={title} className={done ? "done" : index === step ? "current" : ""} aria-current={index === step ? "step" : undefined}>
        {done ? <CheckCircle size={27} weight="fill" /> : <Circle size={27} weight={index === step ? "duotone" : "regular"} />}<span>{title}</span>
      </li>;
    })}</ol>
    <p className="runtime-install-caption">{isChat ? "只安装缺少的组件，已安装的兼容组件继续使用。" : "安装供 CrewVela 使用，外部终端保留原 CLI；账号额度需另行确认。"}</p>
    <section className="runtime-install-progress" aria-label="安装进度">
      <h3>{heading}</h3>
      {busy && <div className="runtime-install-progress-row"><progress aria-label="下载进度" max={100} value={percent} />{percent !== undefined && <strong>{percent}%</strong>}</div>}
      {phaseMessage && <p role="status">{phaseMessage}</p>}
      {(error || statusError) && <p role="alert" className="runtime-install-error"><WarningCircle size={18} />{error || statusError}</p>}
      {failed && <p role="alert" className="runtime-install-error"><WarningCircle size={18} />{job.error}</p>}
      {isChat && !busy && !loading && !installed && !chat?.canInstall && environment?.error && <p className="runtime-install-note">{environment.error}</p>}
      {latestError && !busy && <p className="runtime-install-note">官方版本暂未确认：{latestError}</p>}
    </section>
    {isChat ? <dl className="runtime-install-facts runtime-chat-install-facts">
      <div><dt>{chat?.cliLabel ?? "运行程序"}</dt><dd>{loading && !chat ? "正在检查…" : environment?.executable ? <><CheckCircle size={18} />{environment.version ?? "已检测到"}</> : <><WarningCircle size={18} className="runtime-install-missing" />未检测到</>}</dd></div>
      {chat?.nodeRequired && <div><dt>Node.js</dt><dd>{chat.nodeVersion ? <><CheckCircle size={18} />{chat.nodeVersion} · 继续使用</> : <><WarningCircle size={18} className="runtime-install-missing" />未检测到兼容版本<small>安装 24 LTS，供需要 Node.js 的 Chat 共用</small></>}</dd></div>}
      {chat?.adapterRequired && <div><dt>聊天适配器</dt><dd>{chat.adapterInstalled ? "已检测到" : chat.canInstall ? "将自动安装" : "未检测到"}</dd></div>}
      <div><dt>登录状态</dt><dd>{installed ? authLabel : "验证后检查"}</dd></div>
    </dl> : <dl className="runtime-install-facts">
      <div><dt>依赖检查</dt><dd>{loading ? "正在检查…" : !environment ? "检查未完成，请重新检查" : <>{environment.supported && <CheckCircle size={18} />}{environment.dependencyMessage ?? (environment.supported ? "当前平台支持，不需要 Node/npm" : "此平台请使用官方安装方式")}</>}</dd></div>
      <div><dt>安装方式</dt><dd>{info.method} · 当前用户目录</dd></div>
      <div><dt>原有配置</dt><dd>沿用本地配置</dd></div>
      {!busy && <><div><dt>版本</dt><dd>{loading ? "正在确认…" : environment?.version ?? "尚未确认"}<span className="runtime-install-latest">官方版本 {latestVersion ?? (latestLoading ? "正在确认…" : "尚未确认")}</span></dd></div>
      <div><dt>登录状态</dt><dd>{loading ? "正在检查…" : environment?.auth === "authenticated" ? "已登录（未验证额度）" : environment?.auth === "login_required" ? "待登录" : "未验证，请在原生 CLI 中确认"}</dd></div></>}
    </dl>}
    <details className="runtime-install-details"><summary><CaretDown size={17} />安装详情</summary><dl>
      <div><dt>当前来源</dt><dd>{environment?.source ?? "正在检测…"}</dd></div>
      <div><dt>当前路径</dt><dd>{environment?.executable ?? (loading ? "正在检测…" : "未检测到可用 CLI")}</dd></div>
      <div><dt>安装目录</dt><dd>{environment?.installDirectory ?? "正在检测…"}</dd></div>
      {environment?.error && <div><dt>当前安装检查</dt><dd>{environment.error}</dd></div>}
      <div><dt>文件校验</dt><dd>{isChat ? "Node.js 官方 SHA256 · npm 软件包完整性校验" : info.checksum}</dd></div>
      {!isChat && info.channel && <div><dt>发行渠道</dt><dd>{info.channel}</dd></div>}
      {!isChat && busy && <div><dt>目标版本</dt><dd>{job?.version ?? "正在确认…"}</dd></div>}
    </dl></details>
    {loginOpened && <p className="runtime-install-note" role="status">已打开原生 CLI 登录窗口。{info.loginHint}登录信息由 CLI 自行管理。</p>}
    {isChat ? <footer className="runtime-install-footer">
      <span>{busy ? "关闭面板后继续，可返回查看" : ""}</span>
      {busy ? <button type="button" className="btn secondary" disabled={starting || cancelling} onClick={() => void cancel()}>{cancelling ? "正在取消…" : "取消安装"}</button> : <>
        <button type="button" className="btn secondary" disabled={loading} onClick={() => void refresh()}><ArrowClockwise size={16} />重新检查</button>
        {chatAction}
      </>}
    </footer> : <footer className="runtime-install-footer">
      {busy ? <button className="btn secondary" disabled={starting || cancelling} onClick={() => void cancel()}>{cancelling ? "正在取消…" : "取消安装"}</button>
        : <button className="btn secondary" disabled={loading || !environment?.supported} onClick={() => void start()}>{installAction}</button>}
      <span>{busy ? "关闭面板后继续，可返回查看" : installed ? "安装与登录状态分别确认" : "验证通过后可登录"}</span>
      {!busy && <button className="btn secondary" disabled={loading} onClick={() => { void refresh(); void refreshLatest(); }}><ArrowClockwise size={16} />{kind === "antigravity" ? "检查更新" : "重新检查"}</button>}
      <button className="btn runtime-install-primary" disabled={!installed || busy || loginBusy} onClick={() => void login()}>{loginBusy ? "正在打开…" : info.loginLabel}</button>
    </footer>}
  </div>;
}
