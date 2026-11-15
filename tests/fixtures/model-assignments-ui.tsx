import { createRoot } from "react-dom/client";
import { SettingsDialog } from "../../src/components/SettingsDialog";
import { assignmentPreview, type ModelAssignmentSnapshot } from "../../src/modelAssignments";
import { DEFAULT_APPEARANCE } from "../../src/appearance";
import { DEFAULT_PET_SETTINGS } from "../../src/petSettings";
import { DEFAULT_NOTIFICATION_SETTINGS } from "../../src/notificationSettings";
import type { ProviderConfig } from "../../src/types";
import "../../src/styles.css";

const snapshot = assignmentPreview();
snapshot.rows.push({ ...snapshot.rows.at(-1)!, id:"session:idle",targetId:"idle",name:"已停止的 Codex",editable:true,note:"修改启动覆盖，恢复会话时使用" });
const providers: ProviderConfig[] = [{ id:"glm",name:"智谱服务",baseUrl:"https://fixture.invalid/anthropic",authTokenRef:"fixture",env:{ ANTHROPIC_MODEL:"glm-5.3" },builtin:false,disableNonessentialTraffic:false,timeoutMs:null }];
let sequence = 0;
const pending: { command: string; resolve: () => void; reject: (error: Error) => void }[] = [];
const fixture = { calls: [] as { command: string; args: any }[], hold: [] as string[], failure: false,
  release(command: string, error?: string) { const index=pending.findIndex(item=>item.command===command); if(index<0) throw Error(`No pending ${command}`); const item=pending.splice(index,1)[0]; if(error)item.reject(Error(error));else item.resolve(); }, snapshot, pending };
(window as any).__modelAssignmentMock = fixture;
(window as any).__TAURI_INTERNALS__ = { transformCallback: () => 1, async invoke(command: string, args: any) {
  fixture.calls.push({ command,args });
  const initial = structuredClone(snapshot);
  if (fixture.hold.includes(command)) await new Promise<void>((resolve,reject)=>pending.push({command,resolve,reject}));
  if (command === "model_assignments_snapshot") return initial;
  if (command === "model_assignments_save") {
    if(fixture.failure) throw Error("模拟保存失败");
    const input=args.input; const row=snapshot.rows.find(row=>row.id===input.id)!;
    if(row.version!==input.version) throw Error("配置已发生变化");
    if(!row.editable) throw Error("会话正在运行");
    const raw=input.restore ? { connectionId:null,model:null } : { connectionId:input.connectionId,model:input.model };
    row.raw=raw;
    const defaults=snapshot.rows.find(item=>item.scope==='runtime'&&item.runtime===row.runtime)!;
    row.effective={connectionId:raw.connectionId??(row.scope==='runtime'?'local':defaults.effective.connectionId),model:raw.model??(row.scope==='runtime'?null:defaults.effective.model),source:row.scope==='runtime'?'runtime_default':row.scope==='session'?'session':raw.model||raw.connectionId?'member':'runtime_default'};
    row.version=String(++sequence);
    return structuredClone(snapshot) as ModelAssignmentSnapshot;
  }
  if(command==='get_provider_quota') return {status:'unsupported',updatedAt:null,message:'隔离 Mock',balances:[],windows:[]};
  throw Error(`Unexpected IPC ${command}`);
} };
const params=new URLSearchParams(location.search);
document.documentElement.dataset.theme=params.get('theme')==='light'?'light':'dark';
document.documentElement.dataset.appearanceAccent='none';
createRoot(document.getElementById('root')!).render(<SettingsDialog initialSection="providers" sessions={[]} archivedSessions={[]} projects={[]} providers={providers} keyStates={{glm:true}} usageByProvider={{}}
 appearance={{...DEFAULT_APPEARANCE,accent:'none'}} resolvedAppearanceTheme={params.get('theme')==='light'?'light':'dark'} petSettings={DEFAULT_PET_SETTINGS} notificationSettings={DEFAULT_NOTIFICATION_SETTINGS}
 onAppearanceChange={()=>{}} onPetSettingsChange={()=>{}} onNotificationSettingsChange={()=>{}}  onEdit={()=>{}} onNew={()=>{}} onDelete={()=>{}} onSetHidden={()=>{}} onRestoreDefault={()=>{}} onReorder={async()=>{}} onRestoreArchivedSession={()=>{}} onDeleteArchivedSession={()=>{}} onViewArchivedTranscript={()=>{}} onClose={()=>{}} />);
