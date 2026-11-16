import { createRoot } from "react-dom/client";
import { ClaudeChat } from "../../src/components/ClaudeChat";
import { CodexChat } from "../../src/components/CodexChat";
import { KimiChat } from "../../src/components/KimiChat";
import { OpenCodeChat } from "../../src/components/OpenCodeChat";
import { PiChat } from "../../src/components/PiChat";
import { DeepSeekHarnessChat } from "../../src/components/DeepSeekHarnessChat";
import { ZCodeChat } from "../../src/components/ZCodeChat";
import type { SessionRecord } from "../../src/types";
import "../../src/styles.css";

const components={claude:ClaudeChat,codex:CodexChat,kimi:KimiChat,opencode:OpenCodeChat,pi:PiChat,harness:DeepSeekHarnessChat,zcode:ZCodeChat};
const query=new URLSearchParams(location.search),kind=query.get("kind") as keyof typeof components||"claude";
const pending:Array<{command:string;args:any;resolve:(value:any)=>void;reject:(reason:string)=>void}>=[];
const calls:Array<{command:string;args:any}>=[];
let connected=query.get("connection")!=="hold",callbackId=0,attachments=0;
const callbacks=new Map<number,(event:unknown)=>void>();
const snapshot=()=>({sessionId:kind,connected,busy:false,history:[],pendingPermission:null,toolCalls:[],sequence:0,controlState:{commands:[]},commands:[],configOptions:[],rows:[],models:[{id:"fixture-model",name:"fixture model"}],selectedModelId:"fixture-model",permissionMode:"build",queuedInputs:[],recoverableInputs:[],queueAutoDrain:false,canQueue:true,inputRouting:"startNow"});
const result=(command:string)=>{
  if(command.startsWith("start_"))connected=true;
  if(command.includes("snapshot")||command.startsWith("start_")||command.startsWith("send_"))return snapshot();
  if(command==="zcode_composer_catalog")return {skills:{items:[]},commands:{items:[]},plugins:{items:[]}};
  if(command==="plugin:dialog|open")return [`/attachment-${++attachments}.txt`];
  return null;
};
(window as any).__TAURI_INTERNALS__={
  transformCallback:(fn:(event:unknown)=>void)=>{callbacks.set(++callbackId,fn);return callbackId;},
  invoke:(command:string,args:any)=>{
    calls.push({command,args});
    if(command==="plugin:event|listen")return Promise.resolve(++callbackId);
    if(command==="plugin:event|unlisten")return Promise.resolve(null);
    if((query.get("connection")==="hold"&&command.startsWith("start_"))||command.startsWith("send_"))return new Promise((resolve,reject)=>pending.push({command,args,resolve,reject}));
    return Promise.resolve(result(command));
  },
};
(window as any).__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener(){}};
(window as any).asyncChatMock={calls,pending,release(command:string,failure?:string){
  const index=pending.findIndex(item=>item.command===command);if(index<0)throw Error(`No pending ${command}`);
  const [item]=pending.splice(index,1);if(failure)item.reject(failure);else item.resolve(result(command));
}};
const session={id:kind,kind:"terminal",agentRuntimeKind:kind==="zcode"?"zcode":kind==="harness"?"deepseek_harness":`${kind}_chat`,providerId:`local-runtime-${kind}`,providerName:kind,cwd:"F:/fixture",status:"running",title:kind,ptyId:null,shell:null,claudeSessionId:kind,yolo:false,autoMode:false,permissionMode:null,titleLocked:false,exitCode:null,lastError:null,createdAt:0,updatedAt:0} as SessionRecord;
const Chat=components[kind];
createRoot(document.getElementById("root")!).render(<Chat session={session} preview={query.has("preview")} managedToolbar onSessionRecord={()=>{}}/>);
