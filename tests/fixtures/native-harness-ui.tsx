import { useState } from 'react';
import { createRoot } from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import { DeepSeekHarnessChat } from '../../src/components/DeepSeekHarnessChat';
import type { SessionRecord } from '../../src/types';
import '../../src/styles.css';
const initial = await invoke<SessionRecord>('native_harness_smoke_record');
function Smoke() {
  const [record, setRecord] = useState(initial);
  return <DeepSeekHarnessChat session={record} onSessionRecord={setRecord} />;
}
createRoot(document.getElementById('root')!).render(<Smoke />);
