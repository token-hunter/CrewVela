import { useRef, useState } from "react";
import { X } from "@phosphor-icons/react";
import { RUNTIME_LABELS, RUNTIME_LOGOS } from "../runtimeLogos";
import { CLI_RUNTIMES, isCliRuntime, launchChoices, launchModeLabel, type CliRuntime, type RuntimeLaunchSelection } from "../runtimeLaunch";
import { useRuntimeAvailability } from "../runtimeAvailability";
import { NEW_SESSION_TERMINAL_ORDER_STORAGE_KEY, useRuntimeOrder } from "../runtimePreferences";
import { RuntimeManagementDialog } from "./RuntimeManagementDialog";
import { Modal } from "./ui/Modal";
import "../styles/cli-launch.css";

export function CliLaunchDialog({ initialRuntime, onLaunch, onClose }: {
  initialRuntime: CliRuntime;
  onLaunch: (kind: CliRuntime, selection: RuntimeLaunchSelection) => Promise<boolean>;
  onClose: () => void;
}) {
  const availability = useRuntimeAvailability();
  const [kind, setKind] = useState(initialRuntime);
  const [override, setOverride] = useState<string | null>(null);
  const [saveAsDefault, setSaveAsDefault] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [managing, setManaging] = useState(false);
  const order = useRuntimeOrder(CLI_RUNTIMES, NEW_SESSION_TERMINAL_ORDER_STORAGE_KEY);
  const submitting = useRef(false);
  const defaultMode = availability.prefs.defaultLaunchModes?.[kind] ?? "cli_default";
  const mode = override ?? defaultMode;
  const choice = launchChoices(kind).find(choice => choice.id === mode);
  const close = () => { if (!submitting.current) onClose(); };
  return <Modal className="cli-launch-dialog" ariaLabel="新建 CLI 会话" onClose={close} closeOnOverlayClick>
    <header><h2>新建会话</h2><button type="button" className="icon-button" aria-label="关闭启动设置" disabled={pending} onClick={close}><X size={19} /></button></header>
    <form onSubmit={async event => {
      event.preventDefault();
      if (submitting.current || !choice || !availability.ready || !availability.isRuntimeEnabled(kind)) return;
      submitting.current = true; setPending(true); setError(null);
      try {
        const started = await onLaunch(kind, { mode, saveAsDefault });
        if (!started) setError("尚未启动；选择已保留，可以调整后重试。");
      } catch (reason) { setError(`启动失败：${String(reason)}`); }
      finally { submitting.current = false; setPending(false); }
    }}>
      <label>Agent<span className="cli-launch-agent-field"><img src={RUNTIME_LOGOS[kind]} data-runtime={kind} alt="" /><select aria-label="启动 Agent" value={kind} disabled={pending} onChange={event => { setKind(event.target.value as CliRuntime); setOverride(null); setSaveAsDefault(false); setError(null); }}>
        {CLI_RUNTIMES.map(runtime => <option key={runtime} value={runtime} disabled={!availability.isRuntimeEnabled(runtime)}>{RUNTIME_LABELS[runtime]}</option>)}
      </select></span></label>
      <label>启动模式<select aria-label="本次启动模式" value={mode} disabled={!availability.ready || pending} onChange={event => setOverride(event.target.value)}>
        {!choice && <option value={mode} disabled>不支持的已保存模式：{mode}</option>}
        {launchChoices(kind).map(choice => <option key={choice.id} value={choice.id}>{choice.label}</option>)}
      </select></label>
      <p className="cli-launch-default">当前默认：{launchModeLabel(kind, defaultMode)}</p>
      {choice && <p className="cli-launch-description">{choice.description}</p>}
      <label className="cli-launch-save"><input type="checkbox" checked={saveAsDefault} disabled={pending} onChange={event => setSaveAsDefault(event.target.checked)} />设为 {RUNTIME_LABELS[kind]} 的默认模式</label>
      {(error || availability.error) && <p className="cli-launch-error" role="alert">{error || availability.error}</p>}
      {!availability.ready && <button type="button" className="btn secondary" onClick={() => void availability.refresh()}>重新读取配置</button>}
      <footer><button type="button" className="cli-launch-manage" disabled={pending} onClick={() => setManaging(true)}>管理默认模式</button><button type="button" className="btn secondary" disabled={pending} onClick={close}>取消</button><button type="submit" className="btn" disabled={!choice || !availability.ready || pending || !availability.isRuntimeEnabled(kind)}>{pending ? "正在启动…" : "启动"}</button></footer>
    </form>
    {managing && <RuntimeManagementDialog order={order.order} orderError={order.error} onClose={() => setManaging(false)} onReorder={(kind, target, position) => { if (isCliRuntime(kind) && isCliRuntime(target)) order.reorder(kind, target, position); }} />}
  </Modal>;
}
