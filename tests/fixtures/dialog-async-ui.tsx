import { useState } from "react";
import { createRoot } from "react-dom/client";
import { AddProjectDialog } from "../../src/components/AddProjectDialog";
import { MultiDirPrompt } from "../../src/components/MultiDirPrompt";
import { TranscriptViewer } from "../../src/components/TranscriptViewer";
import { Modal } from "../../src/components/ui/Modal";
import { ModelCombobox } from "../../src/components/ui/ModelCombobox";
import { NotificationOverlay } from "../../src/components/NotificationOverlay";
import type { SessionRecord } from "../../src/types";
import "../../src/styles.css";

const pending: { kind: string; args: any; resolve: (value: any) => void; reject: (reason: Error) => void }[] = [];
const calls: { kind: string; args: any }[] = [];
const listeners = new Map<string, number>();
const callbacks = new Map<number, (event: any) => void>();
let callbackId = 0;
const hold = (kind: string, args: any) => { calls.push({ kind, args }); return new Promise<any>((resolve, reject) => pending.push({ kind, args, resolve, reject })); };
Object.assign(window, { dialogMock: { pending, calls,
  emit(payload: any) { callbacks.get(listeners.get("notification-overlay-updated")!)?.({ payload }); },
  release(kind: string, value: any, fail = false) {
    const index = pending.findIndex(item => item.kind === kind);
    const [item] = pending.splice(index, 1);
    fail ? item.reject(Error(value)) : item.resolve(value);
  },
}, __TAURI_INTERNALS__: {
  transformCallback(fn: (event: any) => void) { callbacks.set(++callbackId, fn); return callbackId; },
  invoke: (command: string, args: any) => {
    if (command === "plugin:event|listen") { listeners.set(args.event, args.handler); return Promise.resolve(1); }
    if (command === "plugin:event|unlisten" || command === "notification_overlay_ready") return Promise.resolve(null);
    return hold(command, args);
  },
}, __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener() {} } });
function Fixture() {
  const mode = new URLSearchParams(location.search).get("mode");
  const [closed, setClosed] = useState(false);
  const [id, setId] = useState("one");
  const [model, setModel] = useState("");
  if (closed) return <p>已关闭</p>;
  if (mode === "notification") return <NotificationOverlay />;
  if (mode === "combo") return <Modal className="editor" onClose={() => setClosed(true)}><ModelCombobox value={model} options={[{ value: "model-one" }, { value: "model-two" }]} onChange={setModel} /><button onClick={() => setClosed(true)}>完成</button></Modal>;
  if (mode === "transcript") return <><button onClick={() => setId(id === "one" ? "two" : "one")}>切换记录</button><TranscriptViewer session={{ id, title: id } as SessionRecord} onClose={() => setClosed(true)} /></>;
  if (mode === "multi") return <MultiDirPrompt count={2} onCancel={() => setClosed(true)} onConfirm={(dirs, options) => { calls.push({ kind: "confirm", args: { dirs, options } }); setClosed(true); }} />;
  return <AddProjectDialog onCancel={() => setClosed(true)} onConfirm={(name, path) => hold("confirm", { name, path })} />;
}
document.body.dataset.theme = "dark";
createRoot(document.getElementById("root")!).render(<Fixture />);
