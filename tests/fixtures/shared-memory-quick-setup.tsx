import React from "react";
import { createRoot } from "react-dom/client";
import { SharedMemorySettingsPanel } from "../../src/components/SharedMemorySettingsPanel";
import "../../src/styles.css";
const root = createRoot(document.getElementById("root")!);
(window as unknown as { unmountQuickFixture: () => void }).unmountQuickFixture = () => root.unmount();
root.render(<SharedMemorySettingsPanel />);
